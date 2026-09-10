---
title: Pipeline override constants replace shader-def constants
pull_requests: []
---

The shader defs `SCREEN_SPACE_SPECULAR_TRANSMISSION_BLUR_TAPS`, `SLICE_COUNT`, `SAMPLES_PER_SLICE_SIDE` and `SHADOW_SAMPLES` are no longer set. The values are now WGSL `override` declarations supplied through the `constants` field of `VertexState`, `FragmentState` and `ComputePipelineDescriptor`:

- `bevy_pbr::transmission` declares `override SCREEN_SPACE_SPECULAR_TRANSMISSION_BLUR_TAPS: i32 = 8;`
- `bevy_pbr`'s `ssao.wesl` declares `override SLICE_COUNT: u32;` and `override SAMPLES_PER_SLICE_SIDE: u32;`
- `bevy_ui_render`'s `box_shadow.wesl` declares `override SAMPLES: i32;`

The SSAO and box shadow overrides live in the stage's own shader, so their `constants` keys are the bare names.

Custom shaders that read the `SCREEN_SPACE_SPECULAR_TRANSMISSION_BLUR_TAPS` shader def with `#{...}` (or guard on it with `#ifdef`) should import the override instead:

```wgsl
// Bevy 0.19
#ifdef SCREEN_SPACE_SPECULAR_TRANSMISSION_BLUR_TAPS
    let num_taps = #{SCREEN_SPACE_SPECULAR_TRANSMISSION_BLUR_TAPS};
#else
    let num_taps = 8;
#endif

// Bevy 0.20
import bevy_pbr::transmission::SCREEN_SPACE_SPECULAR_TRANSMISSION_BLUR_TAPS;
let num_taps = SCREEN_SPACE_SPECULAR_TRANSMISSION_BLUR_TAPS;
```

A `constants` key is the bare override name when the override is declared in the stage's own shader, or the override's full module path when it is declared in an imported module: `pkg::module::NAME` for an embedded module, or `package::path::to::module::NAME` for a module loaded by asset path (`package::shaders::util::TAPS` for `shaders/util.wesl`). `bevy_pbr::SCREEN_SPACE_SPECULAR_TRANSMISSION_BLUR_TAPS` holds the key `"bevy_pbr::transmission::SCREEN_SPACE_SPECULAR_TRANSMISSION_BLUR_TAPS"` for the transmission override. For WESL shaders, keys whose override does not exist in the compiled module are skipped, so a value can be set for a module that is only conditionally imported. WGSL and SPIR-V shaders receive bare keys unchanged.

```rust
FragmentState {
    constants: vec![(bevy_pbr::SCREEN_SPACE_SPECULAR_TRANSMISSION_BLUR_TAPS.into(), 16.0)],
    ..default()
}
```

`bevy_shader::ShaderCache::get` now returns `Arc<CompiledShader<M>>` instead of `Arc<M>`. Use `.module` for the shader module and `.overrides.resolve(&constants)` to map `constants` keys to the identifiers in the compiled module.
