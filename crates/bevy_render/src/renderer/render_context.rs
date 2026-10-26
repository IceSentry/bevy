use crate::diagnostic::internal::DiagnosticsRecorder;
use crate::render_phase::TrackedRenderPass;
use crate::render_resource::{CommandEncoder, RenderPassDescriptor};
use crate::renderer::{wgpu_wrapper, RenderDevice};
use alloc::borrow::Cow;
use bevy_derive::{Deref, DerefMut};
use bevy_ecs::change_detection::Tick;
use bevy_ecs::component::ComponentId;
use bevy_ecs::prelude::*;
use bevy_ecs::query::{QueryData, QueryFilter, QueryState};
use bevy_ecs::system::{
    Deferred, SystemAccess, SystemBuffer, SystemMeta, SystemName, SystemParam,
    SystemParamAccessConflict, SystemParamValidationError,
};
use bevy_ecs::world::unsafe_world_cell::UnsafeWorldCell;
use bevy_ecs::world::DeferredWorld;
#[cfg(feature = "trace")]
use bevy_log::info_span;
#[cfg(not(target_arch = "wasm32"))]
use bevy_tasks::{ComputeTaskPool, Task};
use core::marker::PhantomData;
use wgpu::CommandBuffer;

#[derive(Default)]
struct PendingCommandBuffersInner {
    commands: Vec<PendingCommandBuffer>,
}

enum PendingCommandBuffer {
    Buffer(CommandBuffer),
    #[cfg(not(target_arch = "wasm32"))]
    Finishing(Task<CommandBuffer>),
}

impl PendingCommandBuffer {
    fn from_encoder(encoder: CommandEncoder, name: impl Into<Cow<'static, str>>) -> Self {
        #[cfg(target_arch = "wasm32")]
        {
            let _ = name;
            Self::Buffer(encoder.finish())
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            #[cfg(feature = "trace")]
            let name = name.into();
            #[cfg(not(feature = "trace"))]
            let _ = name;

            Self::Finishing(ComputeTaskPool::get().spawn(async move {
                #[cfg(feature = "trace")]
                let _span = info_span!("finish_command_buffer", system = name.as_ref()).entered();
                encoder.finish()
            }))
        }
    }
}

wgpu_wrapper!(struct WgpuPendingCommandBuffersInner(PendingCommandBuffersInner));

/// A resource that holds command buffers that are pending submission to the render queue.
///
/// Encoders that are pushed here start to finish at once on the [`ComputeTaskPool`].
/// [`PendingCommandBuffers::finish`] waits for them and keeps the order in which they were pushed.
#[derive(Resource)]
pub struct PendingCommandBuffers(WgpuPendingCommandBuffersInner);

impl Default for PendingCommandBuffers {
    fn default() -> Self {
        Self(WgpuPendingCommandBuffersInner::new(
            PendingCommandBuffersInner::default(),
        ))
    }
}

impl PendingCommandBuffers {
    pub fn push(&mut self, buffers: impl IntoIterator<Item = CommandBuffer>) {
        self.0
            .commands
            .extend(buffers.into_iter().map(PendingCommandBuffer::Buffer));
    }

    fn append(&mut self, commands: &mut Vec<PendingCommandBuffer>) {
        self.0.commands.append(commands);
    }

    pub fn push_encoder(&mut self, encoder: CommandEncoder, name: impl Into<Cow<'static, str>>) {
        self.0
            .commands
            .push(PendingCommandBuffer::from_encoder(encoder, name));
    }

    /// Waits for all pending command buffers and returns them in the order in which they were pushed.
    ///
    /// The encoders started to finish on the [`ComputeTaskPool`] when they were pushed. This function
    /// does not finish them again. It only waits for the tasks that are not complete yet.
    pub fn finish(&mut self) -> impl Iterator<Item = CommandBuffer> {
        #[cfg(feature = "trace")]
        let _finish_command_buffers_span = info_span!("finish_command_buffers").entered();

        let commands = self.0.commands.drain(..);

        #[cfg(target_arch = "wasm32")]
        {
            commands.map(|PendingCommandBuffer::Buffer(command_buffer)| command_buffer)
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            // Wait inside a scope and not with `block_on`. While a scope waits, the current thread
            // also runs tasks from the global executor of the `ComputeTaskPool`:
            // - The render thread helps to finish encoders that no thread has started yet.
            // - If the pool has no threads, the finish tasks still run, so the frame does not hang.
            // One task awaits all entries in push order. This keeps the submission order and does
            // not spawn a task for each entry.
            ComputeTaskPool::get()
                .scope(|scope| {
                    scope.spawn(async move {
                        let mut command_buffers = Vec::with_capacity(commands.len());
                        for command in commands {
                            command_buffers.push(match command {
                                PendingCommandBuffer::Buffer(command_buffer) => command_buffer,
                                PendingCommandBuffer::Finishing(task) => task.await,
                            });
                        }
                        command_buffers
                    });
                })
                .into_iter()
                .flatten()
        }
    }

    pub fn is_empty(&self) -> bool {
        self.0.commands.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.commands.len()
    }
}

#[derive(Default)]
struct RenderContextStateInner {
    command_encoder: Option<CommandEncoder>,
    commands: Vec<PendingCommandBuffer>,
    render_device: Option<RenderDevice>,
}

impl RenderContextStateInner {
    fn flush_encoder(&mut self, name: impl Into<Cow<'static, str>>) {
        if let Some(encoder) = self.command_encoder.take() {
            self.commands
                .push(PendingCommandBuffer::from_encoder(encoder, name));
        }
    }
}

wgpu_wrapper!(struct WgpuRenderContextStateInner(RenderContextStateInner));

/// A resource that holds the current render context state, including command encoder and command buffers.
/// This is used internally by the [`RenderContext`] system parameter. When the [`RenderContext`] is
/// dropped at the end of a system, its encoder starts to finish on the [`ComputeTaskPool`]. Implements
/// [`SystemBuffer`] to append the pending command buffers to [`PendingCommandBuffers`] in topological
/// system order.
pub struct RenderContextState(WgpuRenderContextStateInner);

impl Default for RenderContextState {
    fn default() -> Self {
        Self(WgpuRenderContextStateInner::new(
            RenderContextStateInner::default(),
        ))
    }
}

impl RenderContextState {
    fn command_encoder(&mut self, label: &str) -> &mut CommandEncoder {
        let render_device = self
            .0
            .render_device
            .clone()
            .expect("RenderDevice must be set before accessing command_encoder");

        self.0.command_encoder.get_or_insert_with(|| {
            render_device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some(label) })
        })
    }
}

impl SystemBuffer for RenderContextState {
    fn queue(&mut self, _system_meta: &SystemMeta, mut world: DeferredWorld) {
        #[cfg(feature = "trace")]
        let _span =
            info_span!("RenderContextState::apply", system = %_system_meta.name()).entered();

        let inner = &mut *self.0;

        if !inner.commands.is_empty() {
            let mut pending = world.resource_mut::<PendingCommandBuffers>();
            pending.append(&mut inner.commands);
        }

        inner.render_device = None;
    }
}

/// A system parameter that provides access to a command encoder and render device for issuing
/// rendering commands inside any system running beneath the root [`super::RenderGraph`] schedule in the
/// [`super::render_system`] system.
#[derive(SystemParam)]
pub struct RenderContext<'w, 's> {
    state: Deferred<'s, RenderContextState>,
    system_name: SystemName,
    render_device: Res<'w, RenderDevice>,
    diagnostics_recorder: Option<Res<'w, DiagnosticsRecorder>>,
}

impl<'w, 's> RenderContext<'w, 's> {
    fn ensure_device(&mut self) {
        if self.state.0.render_device.is_none() {
            self.state.0.render_device = Some(self.render_device.clone());
        }
    }

    /// Returns the render device associated with this render context.
    pub fn render_device(&self) -> &RenderDevice {
        &self.render_device
    }

    /// Returns the diagnostics recorder, if available.
    pub fn diagnostic_recorder(&self) -> Option<Res<'w, DiagnosticsRecorder>> {
        self.diagnostics_recorder.as_ref().map(Res::clone)
    }

    /// Returns the current command encoder, creating one if it does not already exist.
    pub fn command_encoder(&mut self) -> &mut CommandEncoder {
        self.ensure_device();
        self.state.command_encoder(self.system_name.as_str())
    }

    /// Begins a tracked render pass with the given descriptor.
    pub fn begin_tracked_render_pass<'a>(
        &'a mut self,
        descriptor: RenderPassDescriptor<'_>,
    ) -> TrackedRenderPass<'a> {
        self.ensure_device();

        let command_encoder = self.state.0.command_encoder.get_or_insert_with(|| {
            self.render_device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some(self.system_name.as_str()),
                })
        });

        let render_pass = command_encoder.begin_render_pass(&descriptor);
        TrackedRenderPass::new(&self.render_device, render_pass)
    }

    /// Adds a finished command buffer to be submitted later.
    pub fn add_command_buffer(&mut self, command_buffer: CommandBuffer) {
        self.state.0.flush_encoder(self.system_name.name());
        self.state
            .0
            .commands
            .push(PendingCommandBuffer::Buffer(command_buffer));
    }
}

impl Drop for RenderContext<'_, '_> {
    fn drop(&mut self) {
        self.state.0.flush_encoder(self.system_name.name());
    }
}

/// A system parameter that can be used to explicitly flush pending command buffers to the render queue.
/// This is typically not necessary, as command buffers are automatically flushed at the end of each
/// render system. However, in some cases it may be useful to flush command buffers earlier.
#[derive(SystemParam)]
pub struct FlushCommands<'w> {
    pending: ResMut<'w, PendingCommandBuffers>,
    queue: Res<'w, super::RenderQueue>,
}

impl<'w> FlushCommands<'w> {
    /// Flushes all pending command buffers to the render queue.
    pub fn flush(&mut self) {
        let mut buffers = self.pending.finish().peekable();
        if buffers.peek().is_some() {
            self.queue.submit(buffers);
        }
    }
}

/// The entity corresponding to the current view being rendered.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Deref, DerefMut)]
pub struct CurrentView(pub Entity);

/// A query that fetches components for the entity corresponding to the current view being rendered,
/// as defined by the [`CurrentView`] resource, equivalent to `query.get(current_view.entity())`.
pub struct ViewQuery<'w, 's, D: QueryData, F: QueryFilter = ()> {
    entity: Entity,
    item: D::Item<'w, 's>,
    _filter: PhantomData<F>,
}

impl<'w, 's, D: QueryData, F: QueryFilter> ViewQuery<'w, 's, D, F> {
    #[inline]
    pub fn entity(&self) -> Entity {
        self.entity
    }

    #[inline]
    pub fn into_inner(self) -> D::Item<'w, 's> {
        self.item
    }
}

pub struct ViewQueryState<D: QueryData, F: QueryFilter> {
    resource_id: ComponentId,
    query_state: QueryState<D, F>,
}

// SAFETY: ViewQuery accesses the CurrentView resource (read) and query components.
// Access is properly registered in init_access.
unsafe impl<'a, D: QueryData + 'static, F: QueryFilter + 'static> SystemParam
    for ViewQuery<'a, '_, D, F>
{
    type State = ViewQueryState<D, F>;
    type Item<'w, 's> = ViewQuery<'w, 's, D, F>;

    fn init_state(world: &mut World) -> Self::State {
        ViewQueryState {
            resource_id: Res::<CurrentView>::init_state(world),
            query_state: Query::init_state(world),
        }
    }

    fn init_access(
        state: &Self::State,
        system_meta: &mut SystemMeta,
        system_access: &mut SystemAccess,
    ) -> Result<(), SystemParamAccessConflict> {
        Res::<CurrentView>::init_access(&state.resource_id, system_meta, system_access)?;
        Query::init_access(&state.query_state, system_meta, system_access)?;
        Ok(())
    }

    #[inline]
    unsafe fn get_param<'w, 's>(
        state: &'s mut Self::State,
        system_meta: &SystemMeta,
        world: UnsafeWorldCell<'w>,
        change_tick: Tick,
    ) -> Result<Self::Item<'w, 's>, SystemParamValidationError> {
        // SAFETY: We have registered resource read access in init_access
        let current_view = unsafe {
            Res::<CurrentView>::get_param(&mut state.resource_id, system_meta, world, change_tick)
        }
        .map_err(|_| {
            SystemParamValidationError::skipped::<Self>("CurrentView resource not present")
        })?;

        let entity = current_view.entity();

        // SAFETY: Query state access is properly registered in init_access.
        // The caller ensures the world matches the one used in init_state.
        let query =
            unsafe { Query::get_param(&mut state.query_state, system_meta, world, change_tick) }?;
        let item = query.get_inner(entity).map_err(|_| {
            SystemParamValidationError::skipped::<Self>("Current view entity does not match query")
        })?;

        Ok(ViewQuery {
            entity,
            item,
            _filter: PhantomData,
        })
    }
}

// SAFETY: ViewQuery with ReadOnlyQueryData only reads from the world.
unsafe impl<'w, 's, D: bevy_ecs::query::ReadOnlyQueryData + 'static, F: QueryFilter + 'static>
    bevy_ecs::system::ReadOnlySystemParam for ViewQuery<'w, 's, D, F>
{
}
