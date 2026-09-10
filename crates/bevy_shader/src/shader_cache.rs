use crate::shader::*;
use alloc::borrow::Cow;
use alloc::sync::Arc;
use bevy_asset::AssetId;
use bevy_platform::collections::{hash_map::EntryRef, HashMap, HashSet};
use core::hash::Hash;
use thiserror::Error;
use tracing::debug;
use tracing::warn;

pub(crate) fn wesl_module_path(import_path: &ShaderImport) -> Option<wesl::syntax::ModulePath> {
    match import_path {
        ShaderImport::Custom(name) => {
            let mut segments = name.split("::");
            let package = segments.next().filter(|s| !s.is_empty())?;
            let components = segments
                .map(|s| (!s.is_empty()).then(|| s.to_string()))
                .collect::<Option<Vec<_>>>()?;
            Some(wesl::syntax::ModulePath {
                origin: wesl::syntax::PathOrigin::Package(package.to_string()),
                components,
            })
        }
        // `ModulePath::from_path` would strip anything after a `.` as an extension.
        ShaderImport::AssetPath(path) => {
            let components: Vec<String> = path
                .split('/')
                .filter(|component| !component.is_empty())
                .map(str::to_string)
                .collect();
            (!components.is_empty()).then_some(wesl::syntax::ModulePath {
                origin: wesl::syntax::PathOrigin::Absolute,
                components,
            })
        }
    }
}

fn is_module_not_found(error: &wesl::Error) -> bool {
    match error {
        wesl::Error::ResolveError(wesl::ResolveError::ModuleNotFound(..))
        | wesl::Error::ImportError(wesl::ImportError::ResolveError(
            wesl::ResolveError::ModuleNotFound(..),
        )) => true,
        wesl::Error::Error(diagnostic) => is_module_not_found(&diagnostic.error),
        _ => false,
    }
}

/// Fully composed source code of a shader module, with all shader defs applied.
///
/// This is roughly equivalent to [`wgpu::ShaderSource`](https://docs.rs/wgpu/latest/wgpu/enum.ShaderSource.html),
/// but with less variants and more concrete types instead of [`Cow`].
///
/// This source will be parsed and validated by the renderer.
///
/// Any necessary shader translation (e.g. from WGSL to SPIR-V or vice versa)
/// must be done internally by the renderer.
#[derive(Clone, Debug)]
pub enum ShaderCacheSource<'a> {
    /// SPIR-V module represented as a slice of words.
    SpirV(&'a [u8]),
    /// WGSL module as a string slice.
    Wgsl(String),
}

/// An id of a pipeline, typically in the [`PipelineCache`](https://docs.rs/bevy/latest/bevy/render/render_resource/struct.PipelineCache.html)
/// Typically corresponds to a unique combination of [`Shader`] and [`ShaderDefVal`]s.
pub type CachedPipelineId = usize;

/// A shader module produced by `load_module`, together with the `override` declarations
/// left in it after compilation.
#[derive(Debug)]
pub struct CompiledShader<ShaderModule> {
    /// The module usable by the render device.
    pub module: ShaderModule,
    /// The `override` declarations in `module`, keyed as in a pipeline descriptor's `constants`.
    pub overrides: ShaderOverrides,
}

/// Maps the keys in a pipeline descriptor's `constants` to the identifiers of the
/// `override` declarations in the compiled WGSL.
///
/// Overrides declared in an imported WESL module are mangled by the compiler, and
/// unreferenced overrides are stripped, so the `constants` keys cannot be handed
/// to the GPU API verbatim.
#[derive(Debug)]
pub struct ShaderOverrides {
    shader_path: String,
    /// `None` for sources bevy does not compile itself (WGSL, SPIR-V); keys pass through.
    names: Option<HashMap<String, String>>,
    modules: HashSet<String>,
}

impl ShaderOverrides {
    fn opaque(shader_path: &str) -> Self {
        Self {
            shader_path: shader_path.to_string(),
            names: None,
            modules: HashSet::default(),
        }
    }

    fn from_wesl(
        shader_path: &str,
        compiled: &wesl::CompileResult,
        resolver: &ShaderResolver,
        modules: HashSet<String>,
    ) -> Self {
        use wesl::syntax::{
            Attribute, DeclarationKind, Expression, GlobalDeclaration, LiteralExpression,
        };
        use wesl::{Resolver, SourceMap};
        let mut names = HashMap::default();
        for decl in &compiled.syntax.global_declarations {
            let GlobalDeclaration::Declaration(decl) = decl.node() else {
                continue;
            };
            if decl.kind != DeclarationKind::Override {
                continue;
            }
            let ident = decl.ident.name().to_string();
            // naga matches an `@id(N)` override by `N` rather than by name.
            let id = decl.attributes.iter().find_map(|attr| match attr.node() {
                Attribute::Id(expr) => match expr.node() {
                    Expression::Literal(LiteralExpression::AbstractInt(id)) => Some(id.to_string()),
                    Expression::Literal(LiteralExpression::I32(id)) => Some(id.to_string()),
                    Expression::Literal(LiteralExpression::U32(id)) => Some(id.to_string()),
                    _ => None,
                },
                _ => None,
            });
            let emitted: String = id.as_deref().unwrap_or(&ident).into();
            if let Some(id) = id {
                names.insert(id, emitted.clone());
            }
            // Root declarations are not mangled and so have no sourcemap entry.
            let key = match compiled.sourcemap.get_decl(&ident) {
                Some((path, item)) => format!("{}::{item}", resolver.canonical_path(path)),
                None => ident,
            };
            names.insert(key, emitted);
        }
        Self {
            shader_path: shader_path.to_string(),
            names: Some(names),
            modules,
        }
    }

    /// Resolves descriptor `constants` to the names the compiled module declares,
    /// dropping keys with no matching override.
    pub fn resolve<'a>(&'a self, constants: &'a [(Cow<'static, str>, f64)]) -> Vec<(&'a str, f64)> {
        constants
            .iter()
            .filter_map(|(key, value)| {
                let key = key.as_ref();
                let Some(names) = &self.names else {
                    if key.contains("::") {
                        debug!(
                            "Skipping pipeline constant `{key}`: shader `{}` is not WESL, so the key cannot name an imported module",
                            self.shader_path
                        );
                        return None;
                    }
                    return Some((key, *value));
                };
                if let Some(name) = names.get(key) {
                    return Some((name.as_ref(), *value));
                }
                match key.rsplit_once("::") {
                    Some((module, _)) if !self.modules.contains(module) => debug!(
                        "Skipping pipeline constant `{key}`: module `{module}` is not part of shader `{}`",
                        self.shader_path
                    ),
                    _ => warn!(
                        "Pipeline constant `{key}` has no matching override in shader `{}`; \
                        misspelled or stripped as unused",
                        self.shader_path
                    ),
                }
                None
            })
            .collect()
    }
}

struct ShaderData<ShaderModule> {
    pipelines: HashSet<CachedPipelineId>,
    processed_shaders: HashMap<Box<[ShaderDefVal]>, Arc<CompiledShader<ShaderModule>>>,
    resolved_imports: HashMap<ShaderImport, AssetId<Shader>>,
    dependents: HashSet<AssetId<Shader>>,
}

impl<T> Default for ShaderData<T> {
    fn default() -> Self {
        Self {
            pipelines: Default::default(),
            processed_shaders: Default::default(),
            resolved_imports: Default::default(),
            dependents: Default::default(),
        }
    }
}

/// A cache for shaders and shader imports, with asset state-tracking for
/// waiting to load shaders until all imports are resolved.
///
/// Note that the `RenderDevice` generic parameter is a means by which
/// to avoid a cyclic dependency with `bevy_render`, while also permitting
/// alternative rendering implementations. The actual processing of the
/// shader source into a usable compiled module is left to the renderer.
pub struct ShaderCache<ShaderModule, RenderDevice> {
    device: RenderDevice,
    data: HashMap<AssetId<Shader>, ShaderData<ShaderModule>>,
    load_module: fn(
        &RenderDevice,
        ShaderCacheSource,
        &ValidateShader,
    ) -> Result<ShaderModule, ShaderCacheError>,
    module_path_to_asset_id: HashMap<wesl::syntax::ModulePath, AssetId<Shader>>,
    shaders: HashMap<AssetId<Shader>, Shader>,
    import_path_shaders: HashMap<ShaderImport, AssetId<Shader>>,
    waiting_on_import: HashMap<ShaderImport, Vec<AssetId<Shader>>>,
    missing_import_logged: HashSet<AssetId<Shader>>,
}

/// A compile time shader value definition to be inlined into the shader source.
/// Variant tuples contain the name of the definition, and the value.
#[expect(missing_docs, reason = "Enum variants are self-explanatory")]
#[derive(serde::Serialize, serde::Deserialize, Clone, PartialEq, Eq, Debug, Hash)]
pub enum ShaderDefVal {
    Bool(Cow<'static, str>, bool),
    Int(Cow<'static, str>, i32),
    UInt(Cow<'static, str>, u32),
}

impl From<&'static str> for ShaderDefVal {
    fn from(key: &'static str) -> Self {
        ShaderDefVal::Bool(key.into(), true)
    }
}

impl From<String> for ShaderDefVal {
    fn from(key: String) -> Self {
        ShaderDefVal::Bool(key.into(), true)
    }
}

impl ShaderDefVal {
    /// Returns the value of the define as a string.
    pub fn value_as_string(&self) -> String {
        match self {
            ShaderDefVal::Bool(_, def) => def.to_string(),
            ShaderDefVal::Int(_, def) => def.to_string(),
            ShaderDefVal::UInt(_, def) => def.to_string(),
        }
    }
}

impl<ShaderModule, RenderDevice> ShaderCache<ShaderModule, RenderDevice> {
    /// Creates a new [`ShaderCache`] with the given shader module loading
    /// function. `load_module` is responsible for actually compiling shader
    /// source into a module usable by the render device.
    pub fn new(
        device: RenderDevice,
        load_module: fn(
            &RenderDevice,
            ShaderCacheSource,
            &ValidateShader,
        ) -> Result<ShaderModule, ShaderCacheError>,
    ) -> Self {
        Self {
            device,
            load_module,
            data: Default::default(),
            module_path_to_asset_id: Default::default(),
            shaders: Default::default(),
            import_path_shaders: Default::default(),
            waiting_on_import: Default::default(),
            missing_import_logged: Default::default(),
        }
    }

    /// Attempts to retrieve or create a compiled shader module for the given
    /// shader id and shader definitions.
    ///
    /// The provided `pipeline` is tracked so it may later be reported "dirty"
    /// when a shader is removed or replaced.
    ///
    /// Note that the cache is keyed by `id` and `shader_defs`, meaning providing
    /// the same `shader_defs` in a different order, or with redundancies, will
    /// not result in cache hits, and thus require re-composing the module and
    /// calling `load_module` again.
    pub fn get(
        &mut self,
        pipeline: CachedPipelineId,
        id: AssetId<Shader>,
        shader_defs: &[ShaderDefVal],
    ) -> Result<Arc<CompiledShader<ShaderModule>>, ShaderCacheError> {
        let shader = self
            .shaders
            .get(&id)
            .ok_or(ShaderCacheError::ShaderNotLoaded(id))?;

        let data = self.data.entry(id).or_default();

        // Wesl imports are scanned as both module and item candidates.
        if !matches!(shader.source, Source::Wesl(_)) {
            let n_asset_imports = shader
                .imports
                .iter()
                .filter(|import| matches!(import, ShaderImport::AssetPath(_)))
                .count();
            let n_resolved_asset_imports = data
                .resolved_imports
                .keys()
                .filter(|import| matches!(import, ShaderImport::AssetPath(_)))
                .count();
            if n_asset_imports != n_resolved_asset_imports {
                return Err(ShaderCacheError::ShaderImportNotYetAvailable);
            }
        }

        data.pipelines.insert(pipeline);

        let mut wesl_dependencies: Vec<AssetId<Shader>> = Vec::new();

        let module = match data.processed_shaders.entry_ref(shader_defs) {
            EntryRef::Occupied(entry) => entry.into_mut(),
            EntryRef::Vacant(entry) => {
                debug!(
                    "processing shader {}, with shader defs {:?}",
                    id, shader_defs
                );
                let (shader_source, overrides) = match &shader.source {
                    Source::SpirV(data) => (
                        ShaderCacheSource::SpirV(data.as_ref()),
                        ShaderOverrides::opaque(&shader.path),
                    ),
                    Source::Wesl(_) => {
                        if let Some(module_path) = wesl_module_path(&shader.import_path) {
                            let mut compiler_options = wesl::CompileOptions {
                                imports: true,
                                condcomp: true,
                                ..Default::default()
                            };

                            let mut closure_defs = Vec::new();
                            let mut visited = HashSet::new();
                            let mut to_visit = vec![id];
                            while let Some(dep_id) = to_visit.pop() {
                                if !visited.insert(dep_id) {
                                    continue;
                                }
                                let Some(dep) = self.shaders.get(&dep_id) else {
                                    continue;
                                };
                                if dep_id != id {
                                    closure_defs.extend(dep.shader_defs.iter());
                                }
                                for import in &dep.imports {
                                    if let Some(import_id) = self.import_path_shaders.get(import) {
                                        to_visit.push(*import_id);
                                    }
                                }
                            }
                            let mut constants = alloc::collections::BTreeMap::new();
                            for shader_def in closure_defs
                                .into_iter()
                                .chain(shader_defs.iter())
                                .chain(shader.shader_defs.iter())
                            {
                                match shader_def {
                                    ShaderDefVal::Bool(key, value) => {
                                        compiler_options
                                            .features
                                            .flags
                                            .insert(key.to_string(), (*value).into());
                                    }
                                    ShaderDefVal::Int(key, value) => {
                                        compiler_options
                                            .features
                                            .flags
                                            .insert(key.to_string(), true.into());
                                        constants.insert(key.as_ref(), value.to_string());
                                    }
                                    ShaderDefVal::UInt(key, value) => {
                                        compiler_options
                                            .features
                                            .flags
                                            .insert(key.to_string(), true.into());
                                        constants.insert(key.as_ref(), value.to_string());
                                    }
                                }
                            }
                            let constants_source: String = constants
                                .iter()
                                .map(|(name, value)| format!("const {name} = {value};\n"))
                                .collect();

                            let shader_resolver = ShaderResolver::new(
                                &self.module_path_to_asset_id,
                                &self.shaders,
                                &constants_source,
                            );

                            let compiled = wesl::compile_sourcemap(
                                &module_path,
                                &shader_resolver,
                                &wesl::EscapeMangler,
                                &compiler_options,
                            )
                            .map_err(|error| {
                                // We use render_plain to avoid rendering ANSI codes
                                // Workaround for: https://github.com/tokio-rs/tracing/issues/3378
                                if is_module_not_found(&error) {
                                    if self.missing_import_logged.insert(id) {
                                        warn!(
                                            "Shader `{}` has an unresolved import:\n{}",
                                            shader.path,
                                            error.diagnostic().render_plain()
                                        );
                                    }
                                    ShaderCacheError::ShaderImportNotYetAvailable
                                } else {
                                    ShaderCacheError::ProcessShaderError(
                                        error.diagnostic().render_plain(),
                                    )
                                }
                            })?;

                            let mut modules = HashSet::default();
                            for used in &compiled.modules {
                                let used = wesl::Resolver::canonical_path(&shader_resolver, used);
                                if let Some(dep_id) = self.module_path_to_asset_id.get(&used)
                                    && *dep_id != id
                                {
                                    wesl_dependencies.push(*dep_id);
                                }
                                modules.insert(used.to_string());
                            }

                            let overrides = ShaderOverrides::from_wesl(
                                &shader.path,
                                &compiled,
                                &shader_resolver,
                                modules,
                            );
                            (ShaderCacheSource::Wgsl(compiled.to_string()), overrides)
                        } else {
                            return Err(ShaderCacheError::ProcessShaderError(format!(
                                "Wesl shader `{}` has a malformed import path `{:?}`",
                                shader.path, shader.import_path
                            )));
                        }
                    }
                    Source::Wgsl(wgsl_source) => (
                        ShaderCacheSource::Wgsl(wgsl_source.to_string()),
                        ShaderOverrides::opaque(&shader.path),
                    ),
                };

                let module =
                    (self.load_module)(&self.device, shader_source, &shader.validate_shader)?;

                entry.insert(Arc::new(CompiledShader { module, overrides }))
            }
        };
        let module = module.clone();

        for dep_id in wesl_dependencies {
            self.data.entry(dep_id).or_default().dependents.insert(id);
        }

        Ok(module)
    }

    fn clear(&mut self, id: AssetId<Shader>) -> Vec<CachedPipelineId> {
        let mut shaders_to_clear = vec![id];
        let mut visited = HashSet::new();
        let mut pipelines_to_queue = Vec::new();
        while let Some(handle) = shaders_to_clear.pop() {
            if !visited.insert(handle) {
                continue;
            }
            if let Some(data) = self.data.get_mut(&handle) {
                data.processed_shaders.clear();
                pipelines_to_queue.extend(data.pipelines.iter().copied());
                shaders_to_clear.extend(data.dependents.iter().copied());
            }
        }

        pipelines_to_queue
    }

    /// Inserts and possibly replaces a shader at the given asset id.
    ///
    /// Returns a vec of which cached pipelines depended on it
    /// (directly or indirectly via a shader import) and thus must be recompiled.
    pub fn set_shader(&mut self, id: AssetId<Shader>, shader: Shader) -> Vec<CachedPipelineId> {
        self.missing_import_logged.clear();
        let mut pipelines_to_queue = self.clear(id);
        let path = &shader.import_path;
        self.import_path_shaders.insert(path.clone(), id);
        if let Some(waiting_shaders) = self.waiting_on_import.get_mut(path) {
            for waiting_shader in waiting_shaders.drain(..) {
                // resolve waiting shader import
                let data = self.data.entry(waiting_shader).or_default();
                data.resolved_imports.insert(path.clone(), id);
                // add waiting shader as dependent of this shader
                let data = self.data.entry(id).or_default();
                data.dependents.insert(waiting_shader);
            }
        }

        for import in shader.imports.iter() {
            if let Some(import_id) = self.import_path_shaders.get(import).copied() {
                // resolve import because it is currently available
                let data = self.data.entry(id).or_default();
                data.resolved_imports.insert(import.clone(), import_id);
                // add this shader as a dependent of the import
                let data = self.data.entry(import_id).or_default();
                data.dependents.insert(id);
            } else {
                let waiting = self.waiting_on_import.entry(import.clone()).or_default();
                waiting.push(id);
            }
        }

        if let Source::Wesl(_) = shader.source {
            match wesl_module_path(&shader.import_path) {
                Some(module_path) => {
                    if let Some(existing) = self.module_path_to_asset_id.get(&module_path).copied()
                        && existing != id
                    {
                        warn!(
                            "Shader module path `{module_path}` was registered to a different \
                            shader, replacing with `{}`",
                            shader.path
                        );
                        pipelines_to_queue.extend(self.clear(existing));
                    }
                    self.module_path_to_asset_id.insert(module_path, id);
                }
                None => warn!(
                    "Shader `{}` has a malformed import path `{:?}`",
                    shader.path, shader.import_path
                ),
            }
        }
        self.shaders.insert(id, shader);
        pipelines_to_queue
    }

    /// Removes the shader with the given asset id.
    ///
    /// Returns a vec of which cached pipelines depended on it
    /// (directly or indirectly via a shader import) and thus must be recompiled.
    pub fn remove(&mut self, id: AssetId<Shader>) -> Vec<CachedPipelineId> {
        let pipelines_to_queue = self.clear(id);
        if let Some(shader) = self.shaders.remove(&id) {
            if self.import_path_shaders.get(&shader.import_path) == Some(&id) {
                self.import_path_shaders.remove(&shader.import_path);
            }
            if let Source::Wesl(_) = shader.source
                && let Some(module_path) = wesl_module_path(&shader.import_path)
                && self.module_path_to_asset_id.get(&module_path) == Some(&id)
            {
                self.module_path_to_asset_id.remove(&module_path);
            }
        }

        pipelines_to_queue
    }
}

/// A Wesl import resolver. Maps module paths to actual Wesl shader source.
pub struct ShaderResolver<'a> {
    module_path_to_asset_id: &'a HashMap<wesl::syntax::ModulePath, AssetId<Shader>>,
    shaders: &'a HashMap<AssetId<Shader>, Shader>,
    constants_source: &'a str,
}

impl<'a> ShaderResolver<'a> {
    /// Creates a shader resolver with the given map of module paths to shader asset ids,
    /// map of shader asset ids to shader source, and the source of the virtual
    /// `constants` module. This resolver is not meant to be long living.
    pub fn new(
        module_path_to_asset_id: &'a HashMap<wesl::syntax::ModulePath, AssetId<Shader>>,
        shaders: &'a HashMap<AssetId<Shader>, Shader>,
        constants_source: &'a str,
    ) -> Self {
        Self {
            module_path_to_asset_id,
            shaders,
            constants_source,
        }
    }
}

impl<'a> wesl::Resolver for ShaderResolver<'a> {
    fn resolve_source(
        &self,
        module_path: &wesl::syntax::ModulePath,
    ) -> Result<Cow<'_, str>, wesl::ResolveError> {
        let module_path = self.canonical_path(module_path);
        if module_path.origin == wesl::syntax::PathOrigin::Package("constants".to_string())
            && module_path.components.is_empty()
        {
            return Ok(Cow::Borrowed(self.constants_source));
        }
        let asset_id = self
            .module_path_to_asset_id
            .get(&module_path)
            .ok_or_else(|| {
                wesl::ResolveError::ModuleNotFound(
                    module_path.clone(),
                    "no shader registered for this module path".to_string(),
                )
            })?;

        let shader = self.shaders.get(asset_id).ok_or_else(|| {
            wesl::ResolveError::ModuleNotFound(
                module_path.clone(),
                "shader asset not loaded".to_string(),
            )
        })?;
        Ok(Cow::Borrowed(shader.source.as_str()))
    }

    fn canonical_path(&self, module_path: &wesl::syntax::ModulePath) -> wesl::syntax::ModulePath {
        match &module_path.origin {
            wesl::syntax::PathOrigin::Package(pkg) if pkg.contains('/') => {
                wesl::syntax::ModulePath {
                    origin: wesl::syntax::PathOrigin::Package(
                        pkg.rsplit('/').next().unwrap().to_string(),
                    ),
                    components: module_path.components.clone(),
                }
            }
            _ => module_path.clone(),
        }
    }

    fn display_name(&self, module_path: &wesl::syntax::ModulePath) -> Option<String> {
        let module_path = self.canonical_path(module_path);
        let asset_id = self.module_path_to_asset_id.get(&module_path)?;
        let shader = self.shaders.get(asset_id)?;
        Some(shader.path.clone())
    }
}

/// Type of error returned by a `PipelineCache` when the creation of a GPU pipeline object failed.
#[expect(missing_docs, reason = "Enum variants are self-explanatory")]
#[derive(Error, Debug)]
pub enum ShaderCacheError {
    #[error(
        "Pipeline could not be compiled because the following shader could not be loaded: {0:?}"
    )]
    ShaderNotLoaded(AssetId<Shader>),
    #[error("Failed to process shader:\n{0}")]
    ProcessShaderError(String),
    #[error("Shader import not yet available.")]
    ShaderImportNotYetAvailable,
    #[error("Could not create shader module: {0}")]
    CreateShaderModule(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_cache() -> ShaderCache<String, ()> {
        ShaderCache::new((), |_, source, _| match source {
            ShaderCacheSource::Wgsl(wgsl) => Ok(wgsl),
            _ => panic!("expected wgsl output"),
        })
    }

    #[test]
    fn import_resolution() {
        let mut cache = test_cache();

        let (maths_id, lighting_id, root_id) = set_test_shaders(&mut cache);

        let compiled = cache
            .get(0, root_id, &[ShaderDefVal::Bool("BRIGHT".into(), true)])
            .unwrap();
        assert!(compiled.module.contains("fn fragment"));
        assert!(compiled.module.contains("* 2.0"));
        assert!(compiled.module.contains("+ 0.1"));

        let (maths, lighting, _) = test_shaders();
        assert!(cache.set_shader(lighting_id, lighting).contains(&0));
        let _ = cache.get(0, root_id, &[ShaderDefVal::Bool("BRIGHT".into(), true)]);
        assert!(cache.set_shader(maths_id, maths).contains(&0));
    }

    #[test]
    fn constants_module() {
        let mut cache = test_cache();
        let mut shader = Shader::from_wesl(
            r#"
@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> scale: f32;
var<uniform> batch: array<vec4<f32>, constants::BATCH_SIZE>;

@fragment
fn fragment() -> @location(0) vec4<f32> {
    return vec4<f32>(scale) + batch[0];
}
"#,
            "shaders/constants_user.wesl",
        );
        shader.shader_defs = vec![ShaderDefVal::UInt("BATCH_SIZE".into(), 4)];
        let id = AssetId::Uuid {
            uuid: bevy_asset::uuid::Uuid::from_u128(7),
        };
        cache.set_shader(id, shader);

        let compiled = cache
            .get(
                0,
                id,
                &[ShaderDefVal::UInt("MATERIAL_BIND_GROUP".into(), 2)],
            )
            .unwrap();
        assert!(compiled.module.contains("= 2;"));
        assert!(compiled.module.contains("= 4;"));
    }

    /// Compiles a root module importing `TAPS` from a library at `lib_path` and checks that `key`
    /// resolves to the mangled name while a key for a module that was not imported is dropped.
    fn assert_imported_override(lib_path: &str, import: &str, key: &str) {
        let mut cache = test_cache();
        let (lib_id, _, root_id) = test_ids();
        let lib = Shader::from_wesl("override TAPS: u32 = 4u;", lib_path);
        let root = Shader::from_wesl(
            format!(
                "import {import};\n\
                 @fragment\n\
                 fn fragment() -> @location(0) vec4<f32> {{ return vec4<f32>(f32(TAPS)); }}"
            ),
            "shaders/root.wesl",
        );
        cache.set_shader(lib_id, lib);
        cache.set_shader(root_id, root);

        let compiled = cache.get(0, root_id, &[]).unwrap();
        let constants = [
            (key.to_string().into(), 8.0),
            ("bevy_other::settings::TAPS".into(), 1.0),
        ];
        let resolved = compiled.overrides.resolve(&constants);
        assert_eq!(resolved.len(), 1);
        let (name, value) = resolved[0];
        assert_eq!(value, 8.0);
        assert_ne!(name, "TAPS");
        assert!(compiled.module.contains(&format!("override {name}")));
    }

    #[test]
    fn imported_override_resolves_to_mangled_name() {
        assert_imported_override(
            "embedded://bevy_lib/settings.wesl",
            "bevy_lib::settings::TAPS",
            "bevy_lib::settings::TAPS",
        );
        assert_imported_override(
            "shaders/util.wesl",
            "super::util::TAPS",
            "package::shaders::util::TAPS",
        );
    }

    #[test]
    fn root_override_resolves_to_bare_name() {
        let mut cache = test_cache();
        let (_, _, root_id) = test_ids();
        let root = Shader::from_wesl(
            r#"
override SCALE: f32 = 1.0;

@fragment
fn fragment() -> @location(0) vec4<f32> {
    return vec4<f32>(SCALE);
}
"#,
            "shaders/root.wesl",
        );
        cache.set_shader(root_id, root);

        let compiled = cache.get(0, root_id, &[]).unwrap();
        let constants = [("SCALE".into(), 2.0)];
        assert_eq!(compiled.overrides.resolve(&constants), vec![("SCALE", 2.0)]);
    }

    #[test]
    fn id_override_resolves_to_id() {
        let mut cache = test_cache();
        let (_, _, root_id) = test_ids();
        let root = Shader::from_wesl(
            r#"
@id(7) override X: f32 = 1.0;

@fragment
fn fragment() -> @location(0) vec4<f32> {
    return vec4<f32>(X);
}
"#,
            "shaders/root.wesl",
        );
        cache.set_shader(root_id, root);

        let compiled = cache.get(0, root_id, &[]).unwrap();
        assert!(compiled.module.contains("@id(7)"));
        let constants = [("X".into(), 2.0), ("7".into(), 2.0)];
        assert_eq!(
            compiled.overrides.resolve(&constants),
            vec![("7", 2.0), ("7", 2.0)]
        );
    }

    #[test]
    fn unreferenced_override_is_dropped() {
        let mut cache = test_cache();
        let (_, _, root_id) = test_ids();
        let root = Shader::from_wesl(
            r#"
override UNUSED: f32 = 1.0;

@fragment
fn fragment() -> @location(0) vec4<f32> {
    return vec4<f32>(1.0);
}
"#,
            "shaders/root.wesl",
        );
        cache.set_shader(root_id, root);

        let compiled = cache.get(0, root_id, &[]).unwrap();
        assert!(!compiled.module.contains("UNUSED"));
        let constants = [("UNUSED".into(), 2.0)];
        assert!(compiled.overrides.resolve(&constants).is_empty());
    }

    #[test]
    fn compile_error_display_names() {
        let mut cache = test_cache();
        let (maths, _, root) = test_shaders();
        let (maths_id, lighting_id, root_id) = test_ids();
        let broken_lighting = Shader::from_wesl(
            "fn brighten(x: f32) -> f32 { return x + ; }",
            "embedded://bevy_pbr/render/lighting.wesl",
        );
        cache.set_shader(maths_id, maths);
        cache.set_shader(lighting_id, broken_lighting);
        cache.set_shader(root_id, root);

        let error = cache
            .get(0, root_id, &[ShaderDefVal::Bool("BRIGHT".into(), true)])
            .expect_err("syntax error");
        let ShaderCacheError::ProcessShaderError(message) = error else {
            panic!("expected ProcessShaderError, got: {error:?}");
        };
        assert!(message.contains("embedded://bevy_pbr/render/lighting.wesl"));
    }

    #[test]
    fn import_retry() {
        let mut cache = test_cache();
        let (maths, lighting, root) = test_shaders();
        let (maths_id, lighting_id, root_id) = test_ids();

        cache.set_shader(root_id, root);
        assert!(matches!(
            cache.get(0, root_id, &[]),
            Err(ShaderCacheError::ShaderImportNotYetAvailable)
        ));

        cache.set_shader(lighting_id, lighting);
        cache.set_shader(maths_id, maths);
        cache.get(0, root_id, &[]).unwrap();
    }

    fn test_shaders() -> (Shader, Shader, Shader) {
        let maths = Shader::from_wesl(
            "fn double(x: f32) -> f32 { return x * 2.0; }",
            "embedded://bevy_render/maths.wesl",
        );
        let lighting = Shader::from_wesl(
            "fn brighten(x: f32) -> f32 { return x + 0.1; }",
            "embedded://bevy_pbr/render/lighting.wesl",
        );
        let root = Shader::from_wesl(
            r#"
import bevy_pbr::render::lighting::brighten;

@fragment
fn fragment() -> @location(0) vec4<f32> {
    var value = bevy_render::maths::double(1.0);
    @if(BRIGHT) { value = brighten(value); }
    return vec4<f32>(value);
}
"#,
            "shaders/root.wesl",
        );
        (maths, lighting, root)
    }

    fn test_ids() -> (AssetId<Shader>, AssetId<Shader>, AssetId<Shader>) {
        let id = |n| AssetId::Uuid {
            uuid: bevy_asset::uuid::Uuid::from_u128(n),
        };
        (id(1), id(2), id(3))
    }

    #[test]
    fn library_def_scoping() {
        let mut cache = test_cache();
        let id = |n| AssetId::Uuid {
            uuid: bevy_asset::uuid::Uuid::from_u128(n),
        };

        let mut lib_a = Shader::from_wesl(
            "var<uniform> batch_a: array<vec4<f32>, constants::BATCH_SIZE>;",
            "embedded://bevy_a/bindings.wesl",
        );
        lib_a.shader_defs = vec![ShaderDefVal::UInt("BATCH_SIZE".into(), 3)];
        let mut lib_b = Shader::from_wesl(
            "var<uniform> batch_b: array<vec4<f32>, constants::BATCH_SIZE>;",
            "embedded://bevy_b/bindings.wesl",
        );
        lib_b.shader_defs = vec![ShaderDefVal::UInt("BATCH_SIZE".into(), 7)];

        let root_a = Shader::from_wesl(
            r#"
import bevy_a::bindings::batch_a;
@fragment
fn fragment() -> @location(0) vec4<f32> { return batch_a[0]; }
"#,
            "shaders/root_a.wesl",
        );
        let root_b = Shader::from_wesl(
            r#"
import bevy_b::bindings::batch_b;
@fragment
fn fragment() -> @location(0) vec4<f32> { return batch_b[0]; }
"#,
            "shaders/root_b.wesl",
        );

        cache.set_shader(id(1), lib_a);
        cache.set_shader(id(2), lib_b);
        cache.set_shader(id(3), root_a);
        cache.set_shader(id(4), root_b);

        let compiled_a = cache.get(0, id(3), &[]).unwrap();
        let compiled_b = cache.get(1, id(4), &[]).unwrap();
        assert!(compiled_a.module.contains("= 3;") && !compiled_a.module.contains("= 7;"));
        assert!(compiled_b.module.contains("= 7;") && !compiled_b.module.contains("= 3;"));
    }

    #[test]
    fn cyclic_import_invalidation() {
        let mut cache = test_cache();
        let id = |n| AssetId::Uuid {
            uuid: bevy_asset::uuid::Uuid::from_u128(n),
        };
        let module_a = Shader::from_wesl(
            "import bevy_cycle::b::from_b;\nfn from_a() -> f32 { return 1.0; }",
            "embedded://bevy_cycle/a.wesl",
        );
        let module_b = Shader::from_wesl(
            "import bevy_cycle::a::from_a;\nfn from_b() -> f32 { return 2.0; }",
            "embedded://bevy_cycle/b.wesl",
        );
        cache.set_shader(id(1), module_a);
        cache.set_shader(id(2), module_b);
        let module_a = Shader::from_wesl(
            "import bevy_cycle::b::from_b;\nfn from_a() -> f32 { return 3.0; }",
            "embedded://bevy_cycle/a.wesl",
        );
        cache.set_shader(id(1), module_a);
    }

    fn set_test_shaders(
        cache: &mut ShaderCache<String, ()>,
    ) -> (AssetId<Shader>, AssetId<Shader>, AssetId<Shader>) {
        let (maths, lighting, root) = test_shaders();
        let (maths_id, lighting_id, root_id) = test_ids();
        cache.set_shader(maths_id, maths);
        cache.set_shader(lighting_id, lighting);
        cache.set_shader(root_id, root);
        (maths_id, lighting_id, root_id)
    }
}
