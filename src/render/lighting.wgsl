// The engine's lighting, as the module `mira::pbr`: the view, the sun and its shadows, sky
// light and haze, at group 0. Shadows, occlusion and reflections are traced where the device
// traces rays, and looked up in the shadow maps where it does not.
#import mira::pbr::core
#ifdef RAY_TRACING
#import mira::pbr::traced
#else
#import mira::pbr::untraced
#endif
