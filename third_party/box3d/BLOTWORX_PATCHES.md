# Local compatibility patches

Upstream identity remains recorded in REVISION.txt. This copy includes the
following intentional changes and must not be treated as an unmodified checkout.

- `src/physics_world.c`, `TreeCollideCallback`: the mover contact query retries
  full result buffers with increasing capacity. The upstream 64-entry local
  buffer could silently truncate a dense mesh's contacts before the application
  callback received them. Ordinary queries retain the stack-buffer path; larger
  results allocate temporary storage and free it after the callback. A full
  buffer is retried even when exactly full, because the API does not distinguish
  an exact fit from truncation. Allocation/count overflow fails explicitly
  instead of continuing with incomplete collision data.

Coverage: `blotworx::physics::tests::dense_mesh_contacts_are_not_truncated`
uses one finely tessellated mesh and requires more than 128 reported contacts,
stable grounding and repeated updates. This exercises both the upstream
per-shape limit and the application's former total-contact limit.
