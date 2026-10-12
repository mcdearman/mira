// The starter level's ground: lines a metre apart, and bolder ones every five, so that
// distances can be judged by eye. A surface shader (see docs/SHADERS.md): the ground is lit
// and shadowed as anything else is, and this only says what colour it is.

// How strongly a point is on a line of a grid with lines `every` metres apart, `width` pixels
// wide whatever the distance.
fn grid_lines(at: vec2<f32>, every: f32, width: f32) -> f32 {
    let cell = at / every;
    let from_line = abs(fract(cell - 0.5) - 0.5) / max(fwidth(cell), vec2<f32>(1e-5));
    return 1.0 - saturate(min(from_line.x, from_line.y) / width);
}

fn surface(s: Surface, in: VertexOutput) -> Surface {
    var out = s;
    let at = in.world_position.xz;
    // Far off the lines crowd together into a shimmer, so they fade before they do.
    let away = distance(view.camera_position.xyz, in.world_position);
    let fine = grid_lines(at, 1.0, 1.0) * 0.3 * (1.0 - smoothstep(18.0, 40.0, away));
    let bold = grid_lines(at, 5.0, 1.4) * 0.7 * (1.0 - smoothstep(45.0, 90.0, away));
    out.albedo = mix(s.albedo, s.albedo * 1.8 + vec3<f32>(0.04), max(fine, bold));
    return out;
}
