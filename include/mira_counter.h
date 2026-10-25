/*
 * mira's plugin interface cut down to what the smallest test plugin uses: the counter in
 * src/plugin/tests.rs (COUNTER), which counts frames on entities with a transform and moves
 * them along X. For a language binding's first description of the interface.
 *
 * Copied from include/mira.h. A test (plugin::tests::the_cut_down_header_lays_the_table_out_
 * as_the_engine_does) checks that its table has every slot the engine fills, in the same
 * place; when mira.h grows, so must this.
 *
 * MiraApi keeps every slot of the full table, in order: a function this file does not
 * describe is a `void *` of the same size, so the ones it does describe are at the offsets
 * the engine fills. Everything else here is copied from mira.h unchanged.
 */
#ifndef MIRA_COUNTER_H
#define MIRA_COUNTER_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define MIRA_ABI_VERSION 1u

/* An entity handle. Stays valid until the entity is despawned; never reused afterwards. */
typedef uint64_t MiraEntity;
#define MIRA_ENTITY_NONE UINT64_MAX
typedef uint32_t MiraComponent;
typedef struct MiraApp MiraApp;       /* opaque: the app being set up */
typedef struct MiraSystem MiraSystem; /* opaque: one run of one system */

/* When in the frame a system runs. */
enum {
    MIRA_STAGE_STARTUP = 0,      /* once, the first time the plugin is loaded */
    MIRA_STAGE_FIRST = 1,
    MIRA_STAGE_PRE_UPDATE = 2,
    MIRA_STAGE_FIXED_UPDATE = 3, /* fixed timestep; may run 0..n times per frame */
    MIRA_STAGE_UPDATE = 4,
    MIRA_STAGE_POST_UPDATE = 5,
    MIRA_STAGE_LAST = 6
};
/* How a system uses a component. READ and WRITE terms yield a pointer per entity, in the
 * order they are listed; WITH and WITHOUT only filter. WRITE marks the value as changed. */
enum {
    MIRA_READ = 0,
    MIRA_WRITE = 1,
    MIRA_WITH = 2,
    MIRA_WITHOUT = 3
};

typedef struct MiraTerm {
    MiraComponent component;
    uint32_t access; /* MIRA_READ, MIRA_WRITE, MIRA_WITH or MIRA_WITHOUT */
} MiraTerm;

typedef void (*MiraSystemFn)(MiraSystem *system, void *user);

typedef struct MiraSystemDesc {
    const char *name; /* unique within the plugin; a reloaded system replaces its namesake */
    size_t name_len;
    uint32_t stage;
    uint32_t reserved; /* set to 0 */
    MiraSystemFn run;
    void *user; /* passed back to `run` */
    const MiraTerm *terms; /* the entities the system visits; may be empty */
    size_t term_count;
} MiraSystemDesc;

/* "mira.Transform": position, rotation (a unit quaternion, x y z w) and scale, relative to the
 * entity's parent. 48 bytes, 16-byte aligned. */
typedef struct MiraTransform {
    float translation[3];
    float _pad0;
#if defined(__cplusplus)
    alignas(16) float rotation[4];
#else
    _Alignas(16) float rotation[4];
#endif
    float scale[3];
    float _pad1;
} MiraTransform;

typedef struct MiraApi {
    uint32_t abi_version;
    uint32_t size;

    void (*log)(uint32_t level, const char *message, size_t len);
    MiraComponent (*component_register)(MiraApp *app, const char *name, size_t name_len,
                                        size_t size, size_t align, void (*drop)(void *value));
    MiraComponent (*component_lookup)(MiraApp *app, const char *name, size_t name_len,
                                      size_t *size, size_t *align);
    void *(*state)(MiraApp *app, const char *name, size_t name_len, size_t size, size_t align);
    int32_t (*system_add)(MiraApp *app, const MiraSystemDesc *desc);
    void *delta_seconds; /* not described here: see mira.h */
    void *elapsed_seconds; /* not described here: see mira.h */
    uint8_t (*query_next)(MiraSystem *system, MiraEntity *entity, void **components);
    void *query_get; /* not described here: see mira.h */
    MiraEntity (*spawn)(MiraSystem *system);
    void *despawn; /* not described here: see mira.h */
    void (*insert)(MiraSystem *system, MiraEntity entity, MiraComponent component,
                   const void *value);
    void *remove; /* not described here: see mira.h */
    void *system_add_query; /* not described here: see mira.h */
    void *query_next_in; /* not described here: see mira.h */
    void *query_get_in; /* not described here: see mira.h */
    void *query_rewind; /* not described here: see mira.h */
    void *key_down; /* not described here: see mira.h */
    void *key_pressed; /* not described here: see mira.h */
    void *key_released; /* not described here: see mira.h */
    void *mouse_down; /* not described here: see mira.h */
    void *mouse_pressed; /* not described here: see mira.h */
    void *mouse_motion; /* not described here: see mira.h */
    void *mesh_shape; /* not described here: see mira.h */
    void *mesh_create; /* not described here: see mira.h */
    void *set_mesh; /* not described here: see mira.h */
    void *set_material; /* not described here: see mira.h */
    void *event_register; /* not described here: see mira.h */
    void *event_send; /* not described here: see mira.h */
    void *event_next; /* not described here: see mira.h */
    void *set_camera; /* not described here: see mira.h */
    void *set_light; /* not described here: see mira.h */
    void *set_ambient; /* not described here: see mira.h */
    void *set_window_title; /* not described here: see mira.h */
    void *set_collider; /* not described here: see mira.h */
    void *set_body; /* not described here: see mira.h */
    void *apply_impulse; /* not described here: see mira.h */
    void *set_velocity; /* not described here: see mira.h */
    void *velocity; /* not described here: see mira.h */
    void *raycast; /* not described here: see mira.h */
    void *component_describe; /* not described here: see mira.h */
    void *image_load; /* not described here: see mira.h */
    void *set_textures; /* not described here: see mira.h */
    void *spawn_model; /* not described here: see mira.h */
    void *set_parent; /* not described here: see mira.h */
    void *despawn_tree; /* not described here: see mira.h */
    void *spawn_prefab; /* not described here: see mira.h */
    void (*system_fail)(MiraSystem *system, const char *message, size_t len, const char *trace,
                        size_t trace_len);
    void *signal_set; /* not described here: see mira.h */
    void *signal_get; /* not described here: see mira.h */
    void *signal_define; /* not described here: see mira.h */
    void *set_camera_orthographic; /* not described here: see mira.h */
    void *system_order; /* not described here: see mira.h */
    void *sound_tone; /* not described here: see mira.h */
    void *sound_load; /* not described here: see mira.h */
    void *sound_play; /* not described here: see mira.h */
    void *sound_stop; /* not described here: see mira.h */
    void *sound_playing; /* not described here: see mira.h */
    void *set_sound_source; /* not described here: see mira.h */
    void *set_listener; /* not described here: see mira.h */
    void *set_master_volume; /* not described here: see mira.h */
    void *require; /* not described here: see mira.h */
    void *voxel_block; /* not described here: see mira.h */
    void *voxel_get; /* not described here: see mira.h */
    void *voxel_set; /* not described here: see mira.h */
    void *voxel_solid; /* not described here: see mira.h */
    void *voxel_raycast; /* not described here: see mira.h */
    void *set_chunk_viewer; /* not described here: see mira.h */
    void *set_cursor_grabbed; /* not described here: see mira.h */
    void *cursor_grabbed; /* not described here: see mira.h */
    void *quit; /* not described here: see mira.h */
    void *image_create; /* not described here: see mira.h */
    void *set_alpha_cutoff; /* not described here: see mira.h */
    void *sound_create; /* not described here: see mira.h */
    void *setting; /* not described here: see mira.h */
    void *action_bind; /* not described here: see mira.h */
    void *action_bind_axis; /* not described here: see mira.h */
    void *action_state; /* not described here: see mira.h */
    void *action_value; /* not described here: see mira.h */
    void *file_write; /* not described here: see mira.h */
    void *file_read; /* not described here: see mira.h */
    void *pointer_on_ground; /* not described here: see mira.h */
} MiraApi;

/* Expands a string literal to the (pointer, length) pair the API takes. */
#define MIRA_STR(literal) (literal), (sizeof(literal) - 1)

#if defined(_WIN32)
#define MIRA_EXPORT __declspec(dllexport)
#else
#define MIRA_EXPORT __attribute__((visibility("default")))
#endif

#ifdef __cplusplus
}
#endif

#endif /* MIRA_COUNTER_H */