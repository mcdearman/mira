// What each view sees, decided on the card: the frame's view, and each of the sun's shadow
// cascades. Every instance is looked at once; those a view sees are listed batch by batch,
// and each batch's draw is told how many there are.

struct Place {
    // The first three rows of the model matrix; the fourth is 0 0 0 1.
    rows: array<vec4<f32>, 3>,
};

struct Sight {
    // The mesh's bounding sphere in its own space; a negative radius is one not known yet.
    center: vec3<f32>,
    radius: f32,
    // The distances from the camera the instance is drawn over.
    seen: vec2<f32>,
    batch: u32,
    // Bit 0: there is such an instance. Bit 1: it casts a shadow.
    flags: u32,
};

// A draw as the card reads it for an indirect draw. `first_instance` is where the batch's
// stretch of `seen` begins for the view, and is set before this runs.
struct Draw {
    index_count: u32,
    instance_count: atomic<u32>,
    first_index: u32,
    base_vertex: i32,
    first_instance: u32,
};

struct Views {
    // xyz: the camera, w: how far from it a caster still matters
    camera: vec4<f32>,
    // x: where the views of the lights that cast shadows begin (the frame's is first, then
    // the sun's cascades), y: how many batches a view has draws for, z: how many instances
    // to go through, w: which of the views there are this frame, a bit for each
    counts: vec4<u32>,
    // Five planes a view: four sides and the far end, pointing inward.
    planes: array<vec4<f32>, 65>,
};

@group(0) @binding(0) var<storage, read> places: array<Place>;
@group(0) @binding(1) var<storage, read> sights: array<Sight>;
@group(0) @binding(2) var<uniform> views: Views;
@group(0) @binding(3) var<storage, read_write> draws: array<Draw>;
@group(0) @binding(4) var<storage, read_write> seen: array<u32>;

fn inside(view: u32, center: vec3<f32>, radius: f32) -> bool {
    for (var i = 0u; i < 5u; i++) {
        let plane = views.planes[view * 5u + i];
        if dot(plane.xyz, center) + plane.w < -radius {
            return false;
        }
    }
    return true;
}

fn list(view: u32, batch: u32, number: u32) {
    let draw = view * views.counts.y + batch;
    let place = atomicAdd(&draws[draw].instance_count, 1u);
    seen[draws[draw].first_instance + place] = number;
}

@compute @workgroup_size(64)
fn cull(@builtin(global_invocation_id) id: vec3<u32>) {
    let number = id.x;
    if number >= views.counts.z {
        return;
    }
    let sight = sights[number];
    if (sight.flags & 1u) == 0u {
        return;
    }
    let rows = places[number].rows;
    let origin = vec3<f32>(rows[0].w, rows[1].w, rows[2].w);
    // A level of detail is drawn over its own distances, measured to where the entity is.
    let away = distance(views.camera.xyz, origin);
    if away < sight.seen.x || away >= sight.seen.y {
        return;
    }
    // Until the mesh's size is known the instance is taken to be seen.
    let sized = sight.radius >= 0.0;
    let local = vec4<f32>(sight.center, 1.0);
    let center = vec3<f32>(dot(rows[0], local), dot(rows[1], local), dot(rows[2], local));
    let scale = max(
        length(vec3<f32>(rows[0].x, rows[1].x, rows[2].x)),
        max(length(vec3<f32>(rows[0].y, rows[1].y, rows[2].y)), length(vec3<f32>(rows[0].z, rows[1].z, rows[2].z))),
    );
    let radius = sight.radius * scale;

    if !sized || inside(0u, center, radius) {
        list(0u, sight.batch, number);
    }
    if (sight.flags & 2u) == 0u {
        return;
    }
    // Into each shadow map it falls in, and no other: the sun's cascades, for what is near
    // enough the camera to matter to them, and then the lights'.
    let near = !sized || distance(center, views.camera.xyz) - radius <= views.camera.w;
    for (var view = 1u; view < 13u; view++) {
        if (views.counts.w & (1u << view)) == 0u || (view < views.counts.x && !near) {
            continue;
        }
        if !sized || inside(view, center, radius) {
            list(view, sight.batch, number);
        }
    }
}
