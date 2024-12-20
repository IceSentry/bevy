use crate::{App, AppError, Plugin};
use alloc::{
    boxed::Box,
    string::{String, ToString},
    vec::Vec,
};
use bevy_utils::TypeIdMap;
use core::any::TypeId;
use log::{debug, warn};

/// A macro for generating a well-documented [`PluginGroup`] from a list of [`Plugin`] paths.
///
/// Every plugin must implement the [`Default`] trait.
///
/// # Example
///
/// ```
/// # use bevy_app::*;
/// #
/// # mod velocity {
/// #     use bevy_app::*;
/// #     #[derive(Default)]
/// #     pub struct VelocityPlugin;
/// #     impl Plugin for VelocityPlugin { fn build(&self, _: &mut App) {} }
/// # }
/// #
/// # mod collision {
/// #     pub mod capsule {
/// #         use bevy_app::*;
/// #         #[derive(Default)]
/// #         pub struct CapsuleCollisionPlugin;
/// #         impl Plugin for CapsuleCollisionPlugin { fn build(&self, _: &mut App) {} }
/// #     }
/// # }
/// #
/// # #[derive(Default)]
/// # pub struct TickratePlugin;
/// # impl Plugin for TickratePlugin { fn build(&self, _: &mut App) {} }
/// #
/// # mod features {
/// #   use bevy_app::*;
/// #   #[derive(Default)]
/// #   pub struct ForcePlugin;
/// #   impl Plugin for ForcePlugin { fn build(&self, _: &mut App) {} }
/// # }
/// #
/// # mod web {
/// #   use bevy_app::*;
/// #   #[derive(Default)]
/// #   pub struct WebCompatibilityPlugin;
/// #   impl Plugin for WebCompatibilityPlugin { fn build(&self, _: &mut App) {} }
/// # }
/// #
/// # mod audio {
/// #   use bevy_app::*;
/// #   #[derive(Default)]
/// #   pub struct AudioPlugins;
/// #   impl PluginGroup for AudioPlugins {
/// #     fn build(self) -> PluginGroupBuilder {
/// #       PluginGroupBuilder::start::<Self>()
/// #     }
/// #   }
/// # }
/// #
/// # mod internal {
/// #   use bevy_app::*;
/// #   #[derive(Default)]
/// #   pub struct InternalPlugin;
/// #   impl Plugin for InternalPlugin { fn build(&self, _: &mut App) {} }
/// # }
/// #
/// plugin_group! {
///     /// Doc comments and annotations are supported: they will be added to the generated plugin
///     /// group.
///     #[derive(Debug)]
///     pub struct PhysicsPlugins {
///         // If referencing a plugin within the same module, you must prefix it with a colon `:`.
///         :TickratePlugin,
///         // If referencing a plugin within a different module, there must be three colons `:::`
///         // between the final module and the plugin name.
///         collision::capsule:::CapsuleCollisionPlugin,
///         velocity:::VelocityPlugin,
///         // If you feature-flag a plugin, it will be automatically documented. There can only be
///         // one automatically documented feature flag, and it must be first. All other
///         // `#[cfg()]` attributes must be wrapped by `#[custom()]`.
///         #[cfg(feature = "external_forces")]
///         features:::ForcePlugin,
///         // More complicated `#[cfg()]`s and annotations are not supported by automatic doc
///         // generation, in which case you must wrap it in `#[custom()]`.
///         #[custom(cfg(target_arch = "wasm32"))]
///         web:::WebCompatibilityPlugin,
///         // You can nest `PluginGroup`s within other `PluginGroup`s, you just need the
///         // `#[plugin_group]` attribute.
///         #[plugin_group]
///         audio:::AudioPlugins,
///         // You can hide plugins from documentation. Due to macro limitations, hidden plugins
///         // must be last.
///         #[doc(hidden)]
///         internal:::InternalPlugin
///     }
///     /// You may add doc comments after the plugin group as well. They will be appended after
///     /// the documented list of plugins.
/// }
/// ```
#[macro_export]
macro_rules! plugin_group {
    {
        $(#[$group_meta:meta])*
        $vis:vis struct $group:ident {
            $(
                $(#[cfg(feature = $plugin_feature:literal)])?
                $(#[custom($plugin_meta:meta)])*
                $($plugin_path:ident::)* : $plugin_name:ident
            ),*
            $(
                $(,)?$(
                    #[plugin_group]
                    $(#[cfg(feature = $plugin_group_feature:literal)])?
                    $(#[custom($plugin_group_meta:meta)])*
                    $($plugin_group_path:ident::)* : $plugin_group_name:ident
                ),+
            )?
            $(
                $(,)?$(
                    #[doc(hidden)]
                    $(#[cfg(feature = $hidden_plugin_feature:literal)])?
                    $(#[custom($hidden_plugin_meta:meta)])*
                    $($hidden_plugin_path:ident::)* : $hidden_plugin_name:ident
                ),+
            )?

            $(,)?
        }
        $($(#[doc = $post_doc:literal])+)?
    } => {
        $(#[$group_meta])*
        ///
        $(#[doc = concat!(
            " - [`", stringify!($plugin_name), "`](" $(, stringify!($plugin_path), "::")*, stringify!($plugin_name), ")"
            $(, " - with feature `", $plugin_feature, "`")?
        )])*
       $($(#[doc = concat!(
            " - [`", stringify!($plugin_group_name), "`](" $(, stringify!($plugin_group_path), "::")*, stringify!($plugin_group_name), ")"
            $(, " - with feature `", $plugin_group_feature, "`")?
        )]),+)?
        $(
            ///
            $(#[doc = $post_doc])+
        )?
        $vis struct $group;

        impl $crate::PluginGroup for $group {
            fn build(self) -> $crate::PluginGroupBuilder {
                let mut group = $crate::PluginGroupBuilder::start::<Self>();

                $(
                    $(#[cfg(feature = $plugin_feature)])?
                    $(#[$plugin_meta])*
                    {
                        const _: () = {
                            const fn check_default<T: Default>() {}
                            check_default::<$($plugin_path::)*$plugin_name>();
                        };

                        group = group.add(<$($plugin_path::)*$plugin_name>::default());
                    }
                )*
                $($(
                    $(#[cfg(feature = $plugin_group_feature)])?
                    $(#[$plugin_group_meta])*
                    {
                        const _: () = {
                            const fn check_default<T: Default>() {}
                            check_default::<$($plugin_group_path::)*$plugin_group_name>();
                        };

                        group = group.add_group(<$($plugin_group_path::)*$plugin_group_name>::default());
                    }
                )+)?
                $($(
                    $(#[cfg(feature = $hidden_plugin_feature)])?
                    $(#[$hidden_plugin_meta])*
                    {
                        const _: () = {
                            const fn check_default<T: Default>() {}
                            check_default::<$($hidden_plugin_path::)*$hidden_plugin_name>();
                        };

                        group = group.add(<$($hidden_plugin_path::)*$hidden_plugin_name>::default());
                    }
                )+)?

                group
            }
        }
    };
}

/// Combines multiple [`Plugin`]s into a single unit.
///
/// If you want an easier, but slightly more restrictive, method of implementing this trait, you
/// may be interested in the [`plugin_group!`] macro.
pub trait PluginGroup: Sized {
    /// Configures the [`Plugin`]s that are to be added.
    fn build(self) -> PluginGroupBuilder;
    /// Configures a name for the [`PluginGroup`] which is primarily used for debugging.
    fn name() -> String {
        core::any::type_name::<Self>().to_string()
    }
    /// Sets the value of the given [`Plugin`], if it exists
    fn set<T: Plugin>(self, plugin: T) -> PluginGroupBuilder {
        self.build().set(plugin)
    }
}

struct PluginEntry {
    plugin: Box<dyn Plugin>,
    enabled: bool,
}

impl PluginGroup for PluginGroupBuilder {
    fn build(self) -> PluginGroupBuilder {
        self
    }
}

/// Helper method to get the [`TypeId`] of a value without having to name its type.
fn type_id_of_val<T: 'static>(_: &T) -> TypeId {
    TypeId::of::<T>()
}

/// Facilitates the creation and configuration of a [`PluginGroup`].
///
/// Provides a build ordering to ensure that [`Plugin`]s which produce/require a [`Resource`](bevy_ecs::system::Resource)
/// are built before/after dependent/depending [`Plugin`]s. [`Plugin`]s inside the group
/// can be disabled, enabled or reordered.
pub struct PluginGroupBuilder {
    group_name: String,
    plugins: TypeIdMap<PluginEntry>,
    order: Vec<TypeId>,
}

impl PluginGroupBuilder {
    /// Start a new builder for the [`PluginGroup`].
    pub fn start<PG: PluginGroup>() -> Self {
        Self {
            group_name: PG::name(),
            plugins: Default::default(),
            order: Default::default(),
        }
    }

    /// Finds the index of a target [`Plugin`]. Panics if the target's [`TypeId`] is not found.
    fn index_of<Target: Plugin>(&self) -> usize {
        let index = self
            .order
            .iter()
            .position(|&ty| ty == TypeId::of::<Target>());

        match index {
            Some(i) => i,
            None => panic!(
                "Plugin does not exist in group: {}.",
                core::any::type_name::<Target>()
            ),
        }
    }

    // Insert the new plugin as enabled, and removes its previous ordering if it was
    // already present
    fn upsert_plugin_state<T: Plugin>(&mut self, plugin: T, added_at_index: usize) {
        self.upsert_plugin_entry_state(
            TypeId::of::<T>(),
            PluginEntry {
                plugin: Box::new(plugin),
                enabled: true,
            },
            added_at_index,
        );
    }

    // Insert the new plugin entry as enabled, and removes its previous ordering if it was
    // already present
    fn upsert_plugin_entry_state(
        &mut self,
        key: TypeId,
        plugin: PluginEntry,
        added_at_index: usize,
    ) {
        if let Some(entry) = self.plugins.insert(key, plugin) {
            if entry.enabled {
                warn!(
                    "You are replacing plugin '{}' that was not disabled.",
                    entry.plugin.name()
                );
            }
            if let Some(to_remove) = self
                .order
                .iter()
                .enumerate()
                .find(|(i, ty)| *i != added_at_index && **ty == key)
                .map(|(i, _)| i)
            {
                self.order.remove(to_remove);
            }
        }
    }

    /// Sets the value of the given [`Plugin`], if it exists.
    ///
    /// # Panics
    ///
    /// Panics if the [`Plugin`] does not exist.
    pub fn set<T: Plugin>(mut self, plugin: T) -> Self {
        let entry = self.plugins.get_mut(&TypeId::of::<T>()).unwrap_or_else(|| {
            panic!(
                "{} does not exist in this PluginGroup",
                core::any::type_name::<T>(),
            )
        });
        entry.plugin = Box::new(plugin);
        self
    }

    /// Adds the plugin [`Plugin`] at the end of this [`PluginGroupBuilder`]. If the plugin was
    /// already in the group, it is removed from its previous place.
    // This is not confusing, clippy!
    #[expect(
        clippy::should_implement_trait,
        reason = "This does not emulate the `+` operator, but is more akin to pushing to a stack."
    )]
    pub fn add<T: Plugin>(mut self, plugin: T) -> Self {
        let target_index = self.order.len();
        self.order.push(TypeId::of::<T>());
        self.upsert_plugin_state(plugin, target_index);
        self
    }

    /// Adds a [`PluginGroup`] at the end of this [`PluginGroupBuilder`]. If the plugin was
    /// already in the group, it is removed from its previous place.
    pub fn add_group(mut self, group: impl PluginGroup) -> Self {
        let Self {
            mut plugins, order, ..
        } = group.build();

        for plugin_id in order {
            self.upsert_plugin_entry_state(
                plugin_id,
                plugins.remove(&plugin_id).unwrap(),
                self.order.len(),
            );

            self.order.push(plugin_id);
        }

        self
    }

    /// Adds a [`Plugin`] in this [`PluginGroupBuilder`] before the plugin of type `Target`.
    /// If the plugin was already the group, it is removed from its previous place. There must
    /// be a plugin of type `Target` in the group or it will panic.
    pub fn add_before<Target: Plugin>(mut self, plugin: impl Plugin) -> Self {
        let target_index = self.index_of::<Target>();
        self.order.insert(target_index, type_id_of_val(&plugin));
        self.upsert_plugin_state(plugin, target_index);
        self
    }

    /// Adds a [`Plugin`] in this [`PluginGroupBuilder`] after the plugin of type `Target`.
    /// If the plugin was already the group, it is removed from its previous place. There must
    /// be a plugin of type `Target` in the group or it will panic.
    pub fn add_after<Target: Plugin>(mut self, plugin: impl Plugin) -> Self {
        let target_index = self.index_of::<Target>() + 1;
        self.order.insert(target_index, type_id_of_val(&plugin));
        self.upsert_plugin_state(plugin, target_index);
        self
    }

    /// Enables a [`Plugin`].
    ///
    /// [`Plugin`]s within a [`PluginGroup`] are enabled by default. This function is used to
    /// opt back in to a [`Plugin`] after [disabling](Self::disable) it. If there are no plugins
    /// of type `T` in this group, it will panic.
    pub fn enable<T: Plugin>(mut self) -> Self {
        let plugin_entry = self
            .plugins
            .get_mut(&TypeId::of::<T>())
            .expect("Cannot enable a plugin that does not exist.");
        plugin_entry.enabled = true;
        self
    }

    /// Disables a [`Plugin`], preventing it from being added to the [`App`] with the rest of the
    /// [`PluginGroup`]. The disabled [`Plugin`] keeps its place in the [`PluginGroup`], so it can
    /// still be used for ordering with [`add_before`](Self::add_before) or
    /// [`add_after`](Self::add_after), or it can be [re-enabled](Self::enable). If there are no
    /// plugins of type `T` in this group, it will panic.
    pub fn disable<T: Plugin>(mut self) -> Self {
        let plugin_entry = self
            .plugins
            .get_mut(&TypeId::of::<T>())
            .expect("Cannot disable a plugin that does not exist.");
        plugin_entry.enabled = false;
        self
    }

    /// Consumes the [`PluginGroupBuilder`] and [builds](Plugin::build) the contained [`Plugin`]s
    /// in the order specified.
    ///
    /// # Panics
    ///
    /// Panics if one of the plugin in the group was already added to the application.
    #[track_caller]
    pub fn finish(mut self, app: &mut App) {
        for ty in &self.order {
            if let Some(entry) = self.plugins.remove(ty) {
                if entry.enabled {
                    debug!("added plugin: {}", entry.plugin.name());
                    if let Err(AppError::DuplicatePlugin { plugin_name }) =
                        app.add_boxed_plugin(entry.plugin)
                    {
                        panic!(
                            "Error adding plugin {} in group {}: plugin was already added in application",
                            plugin_name,
                            self.group_name
                        );
                    }
                }
            }
        }
    }
}

/// A plugin group which doesn't do anything. Useful for examples:
/// ```
/// # use bevy_app::prelude::*;
/// use bevy_app::NoopPluginGroup as MinimalPlugins;
///
/// fn main(){
///     App::new().add_plugins(MinimalPlugins).run();
/// }
/// ```
#[doc(hidden)]
pub struct NoopPluginGroup;

impl PluginGroup for NoopPluginGroup {
    fn build(self) -> PluginGroupBuilder {
        PluginGroupBuilder::start::<Self>()
    }
}

#[cfg(test)]
mod tests {
    use super::PluginGroupBuilder;
    use crate::{App, NoopPluginGroup, Plugin};

    struct PluginA;
    impl Plugin for PluginA {
        fn build(&self, _: &mut App) {}
    }

    struct PluginB;
    impl Plugin for PluginB {
        fn build(&self, _: &mut App) {}
    }

    struct PluginC;
    impl Plugin for PluginC {
        fn build(&self, _: &mut App) {}
    }

    #[test]
    fn basic_ordering() {
        let group = PluginGroupBuilder::start::<NoopPluginGroup>()
            .add(PluginA)
            .add(PluginB)
            .add(PluginC);

        assert_eq!(
            group.order,
            vec![
                core::any::TypeId::of::<PluginA>(),
                core::any::TypeId::of::<PluginB>(),
                core::any::TypeId::of::<PluginC>(),
            ]
        );
    }

    #[test]
    fn add_after() {
        let group = PluginGroupBuilder::start::<NoopPluginGroup>()
            .add(PluginA)
            .add(PluginB)
            .add_after::<PluginA>(PluginC);

        assert_eq!(
            group.order,
            vec![
                core::any::TypeId::of::<PluginA>(),
                core::any::TypeId::of::<PluginC>(),
                core::any::TypeId::of::<PluginB>(),
            ]
        );
    }

    #[test]
    fn add_before() {
        let group = PluginGroupBuilder::start::<NoopPluginGroup>()
            .add(PluginA)
            .add(PluginB)
            .add_before::<PluginB>(PluginC);

        assert_eq!(
            group.order,
            vec![
                core::any::TypeId::of::<PluginA>(),
                core::any::TypeId::of::<PluginC>(),
                core::any::TypeId::of::<PluginB>(),
            ]
        );
    }

    #[test]
    fn readd() {
        let group = PluginGroupBuilder::start::<NoopPluginGroup>()
            .add(PluginA)
            .add(PluginB)
            .add(PluginC)
            .add(PluginB);

        assert_eq!(
            group.order,
            vec![
                core::any::TypeId::of::<PluginA>(),
                core::any::TypeId::of::<PluginC>(),
                core::any::TypeId::of::<PluginB>(),
            ]
        );
    }

    #[test]
    fn readd_after() {
        let group = PluginGroupBuilder::start::<NoopPluginGroup>()
            .add(PluginA)
            .add(PluginB)
            .add(PluginC)
            .add_after::<PluginA>(PluginC);

        assert_eq!(
            group.order,
            vec![
                core::any::TypeId::of::<PluginA>(),
                core::any::TypeId::of::<PluginC>(),
                core::any::TypeId::of::<PluginB>(),
            ]
        );
    }

    #[test]
    fn readd_before() {
        let group = PluginGroupBuilder::start::<NoopPluginGroup>()
            .add(PluginA)
            .add(PluginB)
            .add(PluginC)
            .add_before::<PluginB>(PluginC);

        assert_eq!(
            group.order,
            vec![
                core::any::TypeId::of::<PluginA>(),
                core::any::TypeId::of::<PluginC>(),
                core::any::TypeId::of::<PluginB>(),
            ]
        );
    }

    #[test]
    fn add_basic_subgroup() {
        let group_a = PluginGroupBuilder::start::<NoopPluginGroup>()
            .add(PluginA)
            .add(PluginB);

        let group_b = PluginGroupBuilder::start::<NoopPluginGroup>()
            .add_group(group_a)
            .add(PluginC);

        assert_eq!(
            group_b.order,
            vec![
                core::any::TypeId::of::<PluginA>(),
                core::any::TypeId::of::<PluginB>(),
                core::any::TypeId::of::<PluginC>(),
            ]
        );
    }

    #[test]
    fn add_conflicting_subgroup() {
        let group_a = PluginGroupBuilder::start::<NoopPluginGroup>()
            .add(PluginA)
            .add(PluginC);

        let group_b = PluginGroupBuilder::start::<NoopPluginGroup>()
            .add(PluginB)
            .add(PluginC);

        let group = PluginGroupBuilder::start::<NoopPluginGroup>()
            .add_group(group_a)
            .add_group(group_b);

        assert_eq!(
            group.order,
            vec![
                core::any::TypeId::of::<PluginA>(),
                core::any::TypeId::of::<PluginB>(),
                core::any::TypeId::of::<PluginC>(),
            ]
        );
    }
}
