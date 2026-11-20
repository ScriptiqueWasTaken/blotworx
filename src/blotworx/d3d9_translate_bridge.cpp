// Blotworx adapter. MojoShader sources are unmodified and retain their license.
#include "mojoshader.h"
#include <stdint.h>
#include <string.h>
extern "C" {
const void *bw_shader_translate(const unsigned char *data, uint32_t size) {
    return MOJOSHADER_parse("glsles3", "main", data, size, NULL, 0, NULL, 0, NULL, NULL, NULL);
}
void bw_shader_translation_free(const void *handle) {
    if (handle) MOJOSHADER_freeParseData((const MOJOSHADER_parseData *)handle);
}
int bw_shader_translation_errors(const void *handle) {
    return handle ? ((const MOJOSHADER_parseData *)handle)->error_count : -1;
}
int bw_shader_has_preshader(const void *handle) {
    return handle && ((const MOJOSHADER_parseData *)handle)->preshader != NULL;
}
int bw_shader_constant_count(const void *handle) {
    return handle ? ((const MOJOSHADER_parseData *)handle)->constant_count : -1;
}
int bw_shader_constant(const void *handle, int slot, int *kind, int *index,
                       uint32_t *words) {
    const MOJOSHADER_parseData *p = (const MOJOSHADER_parseData *)handle;
    if (!p || p->error_count || slot < 0 || slot >= p->constant_count) return 0;
    const MOJOSHADER_constant *c = &p->constants[slot];
    *kind = c->type; *index = c->index;
    memset(words, 0, sizeof(uint32_t) * 4);
    memcpy(words, &c->value, c->type == MOJOSHADER_UNIFORM_BOOL ? sizeof(int) : sizeof(uint32_t) * 4);
    return 1;
}
const char *bw_shader_translation_error(const void *handle, int index) {
    const MOJOSHADER_parseData *p = (const MOJOSHADER_parseData *)handle;
    return p && index >= 0 && index < p->error_count ? p->errors[index].error : NULL;
}
const char *bw_shader_translation_source(const void *handle, uint32_t *size) {
    const MOJOSHADER_parseData *p = (const MOJOSHADER_parseData *)handle;
    if (!p || p->error_count || p->output_len < 0) return NULL;
    *size = (uint32_t)p->output_len; return p->output;
}
int bw_shader_binding_count(const void *handle, int group) {
    const MOJOSHADER_parseData *p = (const MOJOSHADER_parseData *)handle;
    if (!p || p->error_count) return -1;
    switch (group) {
        case 0: return p->uniform_count; case 1: return p->sampler_count;
        case 2: return p->attribute_count; case 3: return p->output_count;
        default: return -1;
    }
}
int bw_shader_binding(const void *handle, int group, int slot, int *kind,
                      int *index, int *count, int *constant, const char **name) {
    const MOJOSHADER_parseData *p = (const MOJOSHADER_parseData *)handle;
    if (slot < 0 || slot >= bw_shader_binding_count(handle, group)) return 0;
    *count = 1; *constant = 0;
    switch (group) {
        case 0: {
            const MOJOSHADER_uniform *u = &p->uniforms[slot];
            *kind=u->type; *index=u->index; *count=u->array_count; *constant=u->constant; *name=u->name; break;
        }
        case 1: {
            const MOJOSHADER_sampler *s = &p->samplers[slot];
            *kind=s->type; *index=s->index; *name=s->name; *constant=s->texbem; break;
        }
        default: {
            const MOJOSHADER_attribute *a = group == 2 ? &p->attributes[slot] : &p->outputs[slot];
            *kind=a->usage; *index=a->index; *name=a->name; break;
        }
    }
    return 1;
}
}
