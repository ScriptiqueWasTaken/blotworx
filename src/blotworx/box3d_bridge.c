#include "box3d/box3d.h"
#include "box3d/collision.h"
#include "box3d/constants.h"
#include "simd.h"
#include "body.h"
#include "physics_world.h"
#include "manifold.h"
#include "contact.h"
#include "shape.h"
#include "broad_phase.h"
#include <stdlib.h>
#include <float.h>
#include <limits.h>
#include <stdio.h>
#include <string.h>

typedef struct { b3CollisionPlane* planes; b3Vec3* points; int* mesh_indices; int* source_triangles; b3SimplexCache* simplexes; float* saved_offsets; int count; int capacity; } BwPlanes;
typedef struct { b3Vec3 linear; b3Vec3 angular; } BwDetachedMotion;
typedef struct { b3MeshData* data; float radius; b3HullData* hull; int* source_triangles; b3ShapeId shape; bool solid; b3Transform transform; int* group_indices; int group_count; bool owns_group; bool retired; bool has_limits; float limits[2]; BwDetachedMotion* detached; b3Capsule capsule; bool is_capsule; } BwStaticMesh;
typedef int (*BwCharacterContact)(int,int,const float*,const float*,float,float*,void*);
typedef int (*BwVelocityLimit)(float,const float*,float*,float*,void*);
typedef int (*BwCharacterMass)(int,int,const float*,float,float*,void*);
typedef float (*BwMaterialMix)(float,uint64_t,float,uint64_t);
typedef struct { b3WorldId world; BwStaticMesh* meshes; int mesh_count; BwPlanes contacts; b3ContactData* touch_contacts; int touch_capacity; b3HullData* mover; float margin; BwCharacterContact character_contact; void* character_context; int character_failed; int query_ignore; int mass_shape; BwCharacterMass mass_policy; void* mass_context; int mass_failed; BwVelocityLimit velocity_limit; void* velocity_context; int velocity_failed; } BwCollision;
// Compound parts share the first part's optional detached motion record.
// Allocate only while a non-static body is outside the world, and free on
// re-entry/teardown. Box3D drops its awake state on Disable; Havok does not.
static BwStaticMesh* body_metadata(BwCollision* c,int index) {
 BwStaticMesh* mesh=&c->meshes[index];
 return &c->meshes[mesh->group_count?mesh->group_indices[0]:index];
}
static bool live_mesh(BwCollision* c,int index) {
 return c && index>=0 && index<c->mesh_count && !c->meshes[index].retired &&
        b3Shape_IsValid(c->meshes[index].shape);
}
static int write_body_velocities(BwCollision* c,int index,b3Vec3 linear,b3Vec3 angular) {
 b3BodyId body=b3Shape_GetBody(c->meshes[index].shape);
 if(!b3Body_IsEnabled(body) && b3Body_GetType(body)!=b3_staticBody) {
  BwStaticMesh* record=body_metadata(c,index);
  if(!record->detached)record->detached=malloc(sizeof(BwDetachedMotion));
  if(!record->detached)return 0;
  record->detached->linear=linear;record->detached->angular=angular;return 1;
 }
 b3Body_SetLinearVelocity(body,linear);b3Body_SetAngularVelocity(body,angular);return 1;
}
static b3ContactMassFcn character_solver_mass;
static bool source_velocity_limits(b3BodyId body,float dt,b3Vec3* linear,b3Vec3* angular,void* context) {
 BwCollision* c=context;
 b3ShapeId shape;
 if(!c->velocity_limit || b3Body_GetShapes(body,&shape,1)!=1)return false;
 uintptr_t index=(uintptr_t)b3Shape_GetUserData(shape);
 if(!index || index>(uintptr_t)c->mesh_count){c->velocity_failed=1;return true;}
 BwStaticMesh* mesh=&c->meshes[index-1];
 if(!mesh->has_limits)return false;
 if(c->velocity_limit(dt,mesh->limits,&linear->x,&angular->x,c->velocity_context)<0)c->velocity_failed=1;
 return true;
}
int bw_collision_velocity_policy(void* handle,BwVelocityLimit policy,void* context) {
 BwCollision* c=handle;if(!c)return 0;
 c->velocity_limit=policy;c->velocity_context=context;c->velocity_failed=0;return 1;
}
int bw_collision_material_policy(void* handle,BwMaterialMix friction,BwMaterialMix restitution) {
 BwCollision* c=handle;if(!c || !friction || !restitution)return 0;
 b3World_SetFrictionCallback(c->world,friction);b3World_SetRestitutionCallback(c->world,restitution);return 1;
}
int bw_collision_contact_events(void* handle,int index) {
 BwCollision* c=handle;if(!live_mesh(c,index))return 0;
 b3Shape_EnableContactEvents(c->meshes[index].shape,true);return 1;
}
typedef int (*BwContactPair)(int,int,void*);
// Reuse storage sized to the solver's current contact capacity. No fixed cap.
int bw_collision_touching_contacts(void* handle,int index,BwContactPair sink,void* context) {
 BwCollision* c=handle;if(!c || index<0 || index>=c->mesh_count || !sink)return 0;
 if(!live_mesh(c,index))return 1;
 b3BodyId body=b3Shape_GetBody(c->meshes[index].shape);
 if(!b3Body_IsEnabled(body))return 1;
 int capacity=b3Body_GetContactCapacity(body);if(capacity<=0)return 1;
 if(capacity>c->touch_capacity) {
  if((size_t)capacity>SIZE_MAX/sizeof(b3ContactData))return 0;
  b3ContactData* data=realloc(c->touch_contacts,(size_t)capacity*sizeof(b3ContactData));
  if(!data)return 0;c->touch_contacts=data;c->touch_capacity=capacity;
 }
 int count=b3Body_GetContactData(body,c->touch_contacts,c->touch_capacity);
 for(int i=0;i<count;i++) {
  b3ContactData* contact=&c->touch_contacts[i];bool touching=false;
  for(int m=0;m<contact->manifoldCount && !touching;m++)
   for(int p=0;p<contact->manifolds[m].pointCount;p++)
    if(contact->manifolds[m].points[p].separation<=B3_LINEAR_SLOP){touching=true;break;}
  if(!touching)continue;
  uintptr_t a=(uintptr_t)b3Shape_GetUserData(contact->shapeIdA),b=(uintptr_t)b3Shape_GetUserData(contact->shapeIdB);
  if(!a || !b || a>(uintptr_t)c->mesh_count || b>(uintptr_t)c->mesh_count)return 0;
  if(!live_mesh(c,(int)a-1) || !live_mesh(c,(int)b-1))continue;
  if(!sink((int)a-1,(int)b-1,context))return 0;
 }
 return 1;
}
int bw_collision_begin_contacts(void* handle,BwContactPair sink,void* context) {
 BwCollision* c=handle;if(!c || !sink)return 0;
 b3ContactEvents events=b3World_GetContactEvents(c->world);
 for(int i=0;i<events.beginCount;i++) {
  b3ContactBeginTouchEvent* event=&events.beginEvents[i];
  if(!b3Shape_IsValid(event->shapeIdA) || !b3Shape_IsValid(event->shapeIdB))return 0;
  uintptr_t a=(uintptr_t)b3Shape_GetUserData(event->shapeIdA),b=(uintptr_t)b3Shape_GetUserData(event->shapeIdB);
  if(!a || !b || a>(uintptr_t)c->mesh_count || b>(uintptr_t)c->mesh_count)return 0;
  if(c->meshes[a-1].retired || c->meshes[b-1].retired ||
     !b3Body_IsEnabled(b3Shape_GetBody(event->shapeIdA)) || !b3Body_IsEnabled(b3Shape_GetBody(event->shapeIdB)))continue;
  if(!sink((int)a-1,(int)b-1,context))return 0;
 }
 return 1;
}
static void character_solver_mass(b3ShapeId a,b3ShapeId b,const b3Manifold* manifolds,int count,
 b3ContactMass* mass_a,b3ContactMass* mass_b,void* context) {
 BwCollision* c=context;if(c->mass_failed || !c->mass_policy)return;
 b3BodyId character=b3Shape_GetBody(c->meshes[c->mass_shape].shape);
 b3BodyId body_a=b3Shape_GetBody(a),body_b=b3Shape_GetBody(b);
 bool first=B3_ID_EQUALS(character,body_a),second=B3_ID_EQUALS(character,body_b);
 if(!first && !second)return;
 uintptr_t other=(uintptr_t)b3Shape_GetUserData(first?b:a);
 if(!other || other>(uintptr_t)c->mesh_count){c->mass_failed=1;return;}
 b3ContactMass* selected=first?mass_a:mass_b;
 const float* inertia=&selected->inverseInertia.cx.x;
 for(int i=0;i<9;i++)if(inertia[i]!=0){c->mass_failed=1;return;}
 // Havok normals point B->A; Box3D normals point A->B. Accumulate
 // in point order without allocating or imposing a contact-count cap.
 float normal[4]={0};
 for(int m=0;m<count;m++)for(int p=0;p<manifolds[m].pointCount;p++) {
  normal[0]-=manifolds[m].normal.x;normal[1]-=manifolds[m].normal.y;normal[2]-=manifolds[m].normal.z;
 }
 float factor=1;
 int result=c->mass_policy(first,(int)other-1,normal,selected->inverseMass,&factor,c->mass_context);
 if(result<0 || !b3IsValidFloat(factor) || factor<0){c->mass_failed=1;return;}
 if(result>0) {
  float effective=selected->inverseMass*factor;
  if(!b3IsValidFloat(effective)){c->mass_failed=1;return;}
  selected->inverseMass=effective;
 }
}
int bw_collision_character_mass_policy(void* handle,int shape,BwCharacterMass policy,void* context) {
 BwCollision* c=handle;if(!c || (policy && !live_mesh(c,shape)))return 0;
 c->mass_shape=shape;c->mass_policy=policy;c->mass_context=context;c->mass_failed=0;
 b3World_SetContactMassCallback(c->world,policy?character_solver_mass:NULL,policy?c:NULL);return 1;
}
static bool query_ignores_shape(BwCollision* c,b3ShapeId shape) {
 if(!c || !b3Shape_IsValid(shape))return true;
 if(!b3Body_IsEnabled(b3Shape_GetBody(shape)))return true;
 uintptr_t index=(uintptr_t)b3Shape_GetUserData(shape);
 if(index>0 && index<=(uintptr_t)c->mesh_count && c->meshes[index-1].retired)return true;
 if(c->query_ignore<0 || !live_mesh(c,c->query_ignore))return false;
 b3BodyId a=b3Shape_GetBody(shape),b=b3Shape_GetBody(c->meshes[c->query_ignore].shape);
 return a.index1==b.index1 && a.world0==b.world0 && a.generation==b.generation;
}
static bool query_ignores_index(BwCollision* c,int index) {
 if(!live_mesh(c,index))return true;
 return query_ignores_shape(c,c->meshes[index].shape);
}
int bw_collision_query_ignore(void* handle,int index) {
 BwCollision* c=handle;if(!c || index< -1 || index>=c->mesh_count)return -2;
 if(index>=0 && !live_mesh(c,index))return -2;
 int previous=c->query_ignore;c->query_ignore=index;return previous;
}
void bw_collision_character_contact_policy(void* handle,BwCharacterContact callback,void* context) {
 BwCollision* c=handle;c->character_contact=callback;c->character_context=context;c->character_failed=0;
}
// Analytic geometry regression entry point; no game asset mutation.
int bw_collision_skin_pair(const float* a,int ac,const float* b,int bc,const float* position,float ra,float rb,float* output) {
 b3HullData* ha=b3CreateHull((const b3Vec3*)a,ac,ac),*hb=b3CreateHull((const b3Vec3*)b,bc,bc);
 if(!ha || !hb){if(ha)b3DestroyHull(ha);if(hb)b3DestroyHull(hb);return -1;}
 b3LocalManifoldPoint points[8];b3LocalManifold manifold={0};manifold.points=points;
 b3Transform t={{position[0],position[1],position[2]},b3Quat_identity};b3SimplexCache cache={0};b3SATCache sat={0};
 b3CollideRoundedHulls(&manifold,8,ha,hb,t,&cache,&sat,b3GetSIMDWidth(),ra,rb);
 if(manifold.pointCount){output[0]=manifold.normal.x;output[1]=manifold.normal.y;output[2]=manifold.normal.z;
  output[3]=points[0].separation;output[4]=points[0].point.x;output[5]=points[0].point.y;output[6]=points[0].point.z;}
 int count=manifold.pointCount;b3DestroyHull(ha);b3DestroyHull(hb);return count;
}
// Regression for a persistent rounded-hull pair crossing between separated
// cores and core overlap. The old union aliases SAT with GJK's simplex count
// and vertex indices and can turn those bytes into out-of-range face indices.
int bw_collision_test_rounded_cache(void) {
 b3BoxHull a=b3MakeBoxHull(0.5f,0.7f,0.4f),b=b3MakeBoxHull(0.45f,0.6f,0.35f);
 b3ContactCache cache={0};
 if((void*)&cache.satCache==(void*)&cache.simplexCache)return 0;
 b3LocalManifoldPoint points[8];b3LocalManifold manifold={0};manifold.points=points;
 int overlapping=0,separated=0;
 for(int repeat=0;repeat<40;repeat++)for(int step=0;step<80;step++) {
  float x=0.7f+0.0125f*(float)(step<40?step:79-step);
  b3Transform transform={{x,0.03f*(float)(repeat%3),0.02f*(float)(repeat%5)},b3Quat_identity};
  b3CollideRoundedHulls(&manifold,8,&a.base,&b.base,transform,&cache.simplexCache,&cache.satCache,
      b3GetSIMDWidth(),0.05f,0.05f);
  if(cache.simplexCache.count>4)return 0;
  for(int i=0;i<cache.simplexCache.count;i++)
   if(cache.simplexCache.indexA[i]>=a.base.vertexCount || cache.simplexCache.indexB[i]>=b.base.vertexCount)return 0;
  if(cache.satCache.type==b3_faceAxisA && cache.satCache.indexA>=a.base.faceCount)return 0;
  if(cache.satCache.type==b3_faceAxisB && cache.satCache.indexB>=b.base.faceCount)return 0;
  if(manifold.pointCount<0 || manifold.pointCount>8)return 0;
  for(int i=0;i<manifold.pointCount;i++)if(!isfinite(manifold.points[i].separation))return 0;
  if(x<0.95f){if(manifold.pointCount==0)return 0;overlapping++;}
  if(x>1.07f){if(manifold.pointCount!=0)return 0;separated++;}
 }
 return overlapping>0 && separated>0;
}
// Simulation adapters. Native motion/filter/activation policy is supplied by
// the Rust world owner; Box3D's solver is not Havok numerical parity.
int bw_collision_body_parameters(void* handle,int index,int mode,const float* values,int awake) {
 BwCollision* c=handle;if(!values || !live_mesh(c,index))return 0;
 if(mode!=2 && mode!=3 && mode!=4 && mode!=5 && mode!=6 && mode!=7)return 0;
 for(int i=0;i<13;i++)if(!b3IsValidFloat(values[i]))return 0;
 if(values[11]<0 || values[12]<0)return 0;
 if(values[6]<0 || values[7]<0 || values[9]<0 || values[10]<0)return 0;
 b3BodyId id=b3Shape_GetBody(c->meshes[index].shape);
 b3Body_SetType(id,mode==5?b3_staticBody:(mode==4?b3_kinematicBody:b3_dynamicBody));
 if(!write_body_velocities(c,index,(b3Vec3){values[0],values[1],values[2]},(b3Vec3){values[3],values[4],values[5]}))return 0;
 b3Body_SetLinearDamping(id,values[6]);b3Body_SetAngularDamping(id,values[7]);
 // EM1: motion7 -> accumulator type2 (802bb918). Gravity kernel
 // 8029e7ac..8029e868 changes linear velocity only for type0; type1/2
 // retain it. Preserve the source record's gravity factor in Rust, but do
 // not add world gravity a second time to controller-driven characters.
 b3Body_SetGravityScale(id,mode==7?0.0f:values[8]);
 BwStaticMesh* mesh=&c->meshes[index];
 int count=mesh->group_count?mesh->group_count:1;
 for(int i=0;i<count;i++) {
  BwStaticMesh* part=&c->meshes[mesh->group_count?mesh->group_indices[i]:index];
  part->has_limits=true;part->limits[0]=values[11];part->limits[1]=values[12];
  b3Shape_SetFriction(part->shape,values[9]);b3Shape_SetRestitution(part->shape,values[10]);
 }
 b3Body_SetAwake(id,awake!=0);c->contacts.count=0;return 1;
}
int bw_collision_body_snapshot(void* handle,int index,float* output) {
 BwCollision* c=handle;if(!output || !live_mesh(c,index))return 0;
 b3BodyId id=b3Shape_GetBody(c->meshes[index].shape);b3WorldTransform t=b3Body_GetTransform(id);
 BwDetachedMotion* detached=body_metadata(c,index)->detached;
 b3Vec3 linear=detached?detached->linear:b3Body_GetLinearVelocity(id),angular=detached?detached->angular:b3Body_GetAngularVelocity(id);
 output[0]=(float)t.p.x;output[1]=(float)t.p.y;output[2]=(float)t.p.z;
 output[3]=t.q.v.x;output[4]=t.q.v.y;output[5]=t.q.v.z;output[6]=t.q.s;
 output[7]=linear.x;output[8]=linear.y;output[9]=linear.z;
 output[10]=angular.x;output[11]=angular.y;output[12]=angular.z;
 b3Pos center=b3Body_GetWorldCenter(id);
 output[13]=(float)center.x;output[14]=(float)center.y;output[15]=(float)center.z;
 return 1;
}
int bw_collision_wake_body(void* handle,int index) {
 BwCollision* c=handle;if(!live_mesh(c,index))return 0;
 b3Body_SetAwake(b3Shape_GetBody(c->meshes[index].shape),true);return 1;
}
// Solver-contact compatibility for NPC gravity admission. This is not Havok's
// checkSupport classifier: only actual touching, walkable, non-separating
// contacts suppress gravity. No player hull or player query filter is reused.
int bw_collision_body_supported(void* handle,int index,float slope_cos) {
 BwCollision* c=handle;if(!live_mesh(c,index))return -1;
 b3BodyId body=b3Shape_GetBody(c->meshes[index].shape);
 int capacity=b3Body_GetContactCapacity(body);if(capacity<=0)return 0;
 b3ContactData* contacts=malloc(sizeof(b3ContactData)*(size_t)capacity);if(!contacts)return -1;
 int count=b3Body_GetContactData(body,contacts,capacity),supported=0;
 for(int i=0;i<count && !supported;i++) {
  const b3ContactData* contact=&contacts[i];
  bool is_a=B3_ID_EQUALS(b3Shape_GetBody(contact->shapeIdA),body);
  b3BodyId other=b3Shape_GetBody(is_a?contact->shapeIdB:contact->shapeIdA);
  for(int j=0;j<contact->manifoldCount && !supported;j++) {
   const b3Manifold* manifold=&contact->manifolds[j];
   b3Vec3 normal=is_a?b3Neg(manifold->normal):manifold->normal;
   if(normal.y<slope_cos)continue;
   for(int p=0;p<manifold->pointCount;p++) {
    const b3ManifoldPoint* point=&manifold->points[p];
    if(point->separation>B3_LINEAR_SLOP)continue;
    b3Pos location=b3OffsetPos(b3Body_GetWorldCenter(body),is_a?point->anchorA:point->anchorB);
    b3Vec3 relative=b3Sub(b3Body_GetWorldPointVelocity(body,location),b3Body_GetWorldPointVelocity(other,location));
    if(b3Dot(relative,normal)<=0){supported=1;break;}
   }
  }
 }
 free(contacts);return supported;
}
int bw_collision_linear_impulse(void* handle,int index,const float* impulse) {
 BwCollision* c=handle;if(!c || !impulse || index<0 || index>=c->mesh_count || c->meshes[index].retired)return 0;
 b3Vec3 v={impulse[0],impulse[1],impulse[2]};if(!b3IsValidVec3(v))return 0;
 b3BodyId body=b3Shape_GetBody(c->meshes[index].shape);
 if(!b3Body_IsEnabled(body))return 0; // Detached native impulse compatibility is not implemented.
 b3Body_ApplyLinearImpulseToCenter(body,v,true);return 1;
}
int bw_collision_body_velocity(void* handle,int index,const float* velocity) {
 BwCollision* c=handle;if(!velocity || !live_mesh(c,index))return 0;
 b3Vec3 v={velocity[0],velocity[1],velocity[2]};if(!b3IsValidVec3(v))return 0;
 b3BodyId body=b3Shape_GetBody(c->meshes[index].shape);
 BwDetachedMotion* detached=body_metadata(c,index)->detached;
 return write_body_velocities(c,index,v,detached?detached->angular:b3Body_GetAngularVelocity(body));
}
int bw_collision_body_velocities(void* handle,int index,const float* linear,const float* angular) {
 BwCollision* c=handle;if(!linear || !angular || !live_mesh(c,index))return 0;
 b3Vec3 lv={linear[0],linear[1],linear[2]},av={angular[0],angular[1],angular[2]};
 if(!b3IsValidVec3(lv) || !b3IsValidVec3(av))return 0;
 b3BodyId body=b3Shape_GetBody(c->meshes[index].shape);
 return write_body_velocities(c,index,lv,av);
}
typedef int (*BwPairFilter)(int,int,void*);
typedef struct {BwPairFilter filter;void* context;} BwSimulationFilter;
static bool simulation_filter(b3ShapeId a,b3ShapeId b,void* context) {
 BwSimulationFilter* f=context;
 uintptr_t first=(uintptr_t)b3Shape_GetUserData(a),second=(uintptr_t)b3Shape_GetUserData(b);
 if(!first || !second || first>INT_MAX || second>INT_MAX)return false;
 return f->filter((int)first-1,(int)second-1,f->context)!=0;
}
// Refilter an installed body without throwing away contacts which remain
// allowed. Merely changing Rust's filter word does not recreate rejected pairs
// for stationary/sleeping bodies. No integration or pose change belongs here.
int bw_collision_refresh_filter(void* handle,int index,BwPairFilter filter,void* context,int* removed) {
 BwCollision* c=handle;
 if(!c || !filter || index<0 || index>=c->mesh_count || c->meshes[index].retired)return 0;
 b3World* world=b3GetUnlockedWorldFromId(c->world);if(!world)return 0;
 b3BodyId id=b3Shape_GetBody(c->meshes[index].shape);
 if(!b3Body_IsEnabled(id))return 0;
 b3Body* body=b3GetBodyFullId(world,id);
 b3CustomFilterFcn* previous=world->customFilterFcn;void* previous_context=world->customFilterContext;
 BwSimulationFilter query={filter,context};
 if(removed)*removed=0;
 world->customFilterFcn=simulation_filter;world->customFilterContext=&query;world->locked=true;
 int key=body->headContactKey;
 while(key!=B3_NULL_INDEX) {
  b3Contact* contact=b3Array_Get(world->contacts,key>>1);
  key=contact->edges[key&1].nextKey;
  b3Shape* a=b3Array_Get(world->shapes,contact->shapeIdA);
  b3Shape* b=b3Array_Get(world->shapes,contact->shapeIdB);
  b3ShapeId aid={a->id+1,world->worldId,a->generation},bid={b->id+1,world->worldId,b->generation};
  if(!simulation_filter(aid,bid,&query)){b3DestroyContact(world,contact,true);if(removed)*removed+=1;}
 }
 for(int shape_index=body->headShapeId;shape_index!=B3_NULL_INDEX;) {
  b3Shape* shape=b3Array_Get(world->shapes,shape_index);shape_index=shape->nextShapeId;
  if(shape->proxyKey!=B3_NULL_INDEX)b3BroadPhase_MarkProxyMovedSerial(&world->broadPhase,shape->proxyKey);
 }
 b3UpdateBroadPhasePairs(world);
 if(world->userTreeTask) {
  world->finishTaskFcn(world->userTreeTask,world->userTaskContext);
  world->userTreeTask=NULL;world->activeTaskCount-=1;
 }
 world->locked=false;world->customFilterFcn=previous;world->customFilterContext=previous_context;
 c->contacts.count=0;return 1;
}
// Compact pair identities for compatibility regression checks, including
// non-touching broadphase contacts before the next integration step.
int bw_collision_pair_ids(void* handle,int index,int* output,int capacity) {
 BwCollision* c=handle;if(!c || index<0 || index>=c->mesh_count || capacity<0)return -1;
 b3World* world=b3GetUnlockedWorldFromId(c->world);if(!world)return -1;
 b3Body* body=b3GetBodyFullId(world,b3Shape_GetBody(c->meshes[index].shape));int count=0;
 for(int key=body->headContactKey;key!=B3_NULL_INDEX;) {
  b3Contact* contact=b3Array_Get(world->contacts,key>>1);key=contact->edges[key&1].nextKey;
  if(output && count<capacity)output[count]=contact->contactId;count++;
 }
 return count;
}
int bw_collision_step(void* handle,float dt,const float* gravity,BwPairFilter filter,void* context) {
 BwCollision* c=handle;if(!c || !gravity || !filter || !b3IsValidFloat(dt) || dt<=0)return 0;
 b3Vec3 g={gravity[0],gravity[1],gravity[2]};if(!b3IsValidVec3(g))return 0;
 BwSimulationFilter f={filter,context};
 b3World_SetGravity(c->world,g);b3World_SetCustomFilterCallback(c->world,simulation_filter,&f);
 // Gameplay advances at 30 Hz, but one constraint solve at that interval
 // cannot reliably resolve the heavy character / movable body / floor stack.
 // Substep only integration and constraints: controller commands, scripts,
 // animation and pose publication still run exactly once per gameplay tick.
 // Keep the solver interval <= 1/120 s, including explicit 60 Hz callers.
 int substeps=(int)ceilf(dt*120.0f);
 if(substeps<1)substeps=1;
 b3World_SetIntegrationVelocityCallback(c->world,c->velocity_limit?source_velocity_limits:NULL,c);
 b3World_Step(c->world,dt,substeps);
 b3World_SetIntegrationVelocityCallback(c->world,NULL,NULL);
 b3World_SetCustomFilterCallback(c->world,NULL,NULL);
 // Manual character/projectile queries share these live transforms with the
 // solver. Copy the body pose, not an independently integrated mesh pose.
 for(int i=0;i<c->mesh_count;i++) {
  BwStaticMesh* mesh=&c->meshes[i];
  // Retired source slots remain as stable tombstones after script deletion.
  // Their Box3D shape/body has been destroyed; never resolve that null ID.
  if(mesh->retired)continue;
  if(!b3Shape_IsValid(mesh->shape))return 0;
  b3BodyId id=b3Shape_GetBody(mesh->shape);
  if(b3Body_GetType(id)==b3_staticBody)continue;
  b3WorldTransform t=b3Body_GetTransform(id);
  mesh->transform=(b3Transform){{(float)t.p.x,(float)t.p.y,(float)t.p.z},t.q};
 }
 c->contacts.count=0;return !c->mass_failed && !c->velocity_failed;
}

// Regression adapter for contact-local mass overrides. It deliberately does
// not interpret the recovered Havok XYZ/W factors as Box3D coefficients.
typedef struct {int shape;float scale;int calls;} BwTestContactMass;
static void test_contact_mass(b3ShapeId a,b3ShapeId b,const b3Manifold* manifolds,int count,
 b3ContactMass* mass_a,b3ContactMass* mass_b,void* context) {
 BwTestContactMass* test=context;
 uintptr_t first=(uintptr_t)b3Shape_GetUserData(a),second=(uintptr_t)b3Shape_GetUserData(b);
 if(first==(uintptr_t)test->shape+1){mass_a->inverseMass*=test->scale;test->calls++;}
 if(second==(uintptr_t)test->shape+1){mass_b->inverseMass*=test->scale;test->calls++;}
 (void)manifolds;(void)count;
}
int bw_collision_test_mass_step(void* handle,float dt,const float* gravity,BwPairFilter filter,
 void* context,int shape,float scale,float* output) {
 BwCollision* c=handle;if(!c || shape<0 || shape>=c->mesh_count || !output
  || !b3IsValidFloat(scale) || scale<0)return 0;
 b3BodyId body=b3Shape_GetBody(c->meshes[shape].shape);
 output[0]=b3Body_GetMassData(body).mass;
 BwTestContactMass test={shape,scale,0};
 b3World_SetContactMassCallback(c->world,test_contact_mass,&test);
 int result=bw_collision_step(handle,dt,gravity,filter,context);
 b3World_SetContactMassCallback(c->world,NULL,NULL);
 output[1]=b3Body_GetMassData(body).mass;output[2]=(float)test.calls;
 return result;
}
// Join imported shape parts without changing their stable query/source IDs.
// This is query-body ownership, not dynamic simulation admission.
int bw_collision_group_shapes(void* handle,const int* indices,int count) {
 BwCollision* c=handle;if(!c || !indices || count<1)return 0;
 for(int i=0;i<count;i++) {
  if(!live_mesh(c,indices[i]))return 0;
  for(int j=0;j<i;j++)if(indices[j]==indices[i])return 0;
 }
 BwStaticMesh* first=&c->meshes[indices[0]];
 for(int i=0;i<count;i++) {
  BwStaticMesh* mesh=&c->meshes[indices[i]];
  b3Transform a=first->transform,b=mesh->transform;
  if(a.p.x!=b.p.x || a.p.y!=b.p.y || a.p.z!=b.p.z || a.q.v.x!=b.q.v.x || a.q.v.y!=b.q.v.y || a.q.v.z!=b.q.v.z || a.q.s!=b.q.s)return 0;
  b3BodyId old=b3Shape_GetBody(mesh->shape);
  if(b3Body_GetShapeCount(old)!=1 || mesh->group_count || mesh->retired || !b3Body_IsEnabled(old))return 0;
 }
 if(count==1)return 1;
 b3ShapeId* shapes=malloc((size_t)count*sizeof(b3ShapeId));if(!shapes)return 0;
 int* members=malloc((size_t)count*sizeof(int));if(!members){free(shapes);return 0;}
 memcpy(members,indices,(size_t)count*sizeof(int));
 b3BodyDef bd=b3DefaultBodyDef();bd.type=b3_staticBody;
 bd.position=(b3Pos){first->transform.p.x,first->transform.p.y,first->transform.p.z};bd.rotation=first->transform.q;
 b3BodyId body=b3CreateBody(c->world,&bd);if(B3_IS_NULL(body)){free(members);free(shapes);return 0;}
 for(int i=0;i<count;i++) {
  BwStaticMesh* mesh=&c->meshes[indices[i]];b3ShapeDef sd=b3DefaultShapeDef();
  sd.convexRadius=mesh->radius;
  sd.userData=(void*)(uintptr_t)(indices[i]+1);sd.filter.categoryBits=mesh->solid?1:2;
  sd.enableCustomFiltering=true;
  shapes[i]=mesh->is_capsule?b3CreateCapsuleShape(body,&sd,&mesh->capsule):mesh->hull?b3CreateHullShape(body,&sd,mesh->hull):b3CreateMeshShape(body,&sd,mesh->data,(b3Vec3){1,1,1});
  if(B3_IS_NULL(shapes[i])){b3DestroyBody(body);free(members);free(shapes);return 0;}
 }
 // Commit only after every replacement shape exists.
 for(int i=0;i<count;i++) {
  BwStaticMesh* mesh=&c->meshes[indices[i]];
  b3DestroyBody(b3Shape_GetBody(mesh->shape));mesh->shape=shapes[i];
  mesh->group_indices=members;mesh->group_count=count;mesh->owns_group=(i==0);
 }
 free(shapes);c->contacts.count=0;return 1;
}
int bw_collision_same_body(void* handle,int a,int b) {
 BwCollision* c=handle;if(!live_mesh(c,a) || !live_mesh(c,b))return 0;
 return B3_ID_EQUALS(b3Shape_GetBody(c->meshes[a].shape),b3Shape_GetBody(c->meshes[b].shape));
}
// Compatibility mass admission: retain authored inverse values, including
// individually locked local axes. Public SetMassData otherwise zeros all
// inverse inertia when its determinant is zero. No force/step policy implied.
int bw_collision_native_mass(void* handle,int index,float inverse_mass,const float* inverse_inertia,const float* center) {
 BwCollision* c=handle;if(!inverse_inertia || !center || !live_mesh(c,index))return 0;
 if(!b3IsValidFloat(inverse_mass) || inverse_mass<0)return 0;
 b3MassData mass={0};mass.mass=inverse_mass==0?0:1/inverse_mass;
 mass.center=(b3Vec3){center[0],center[1],center[2]};if(!b3IsValidVec3(mass.center) || !b3IsValidFloat(mass.mass))return 0;
 float diagonal[3];
 for(int i=0;i<3;i++) {
  if(!b3IsValidFloat(inverse_inertia[i]) || inverse_inertia[i]<0)return 0;
  diagonal[i]=inverse_inertia[i]==0?0:1/inverse_inertia[i];if(!b3IsValidFloat(diagonal[i]))return 0;
 }
 mass.inertia=(b3Matrix3){{diagonal[0],0,0},{0,diagonal[1],0},{0,0,diagonal[2]}};
 b3BodyId id=b3Shape_GetBody(c->meshes[index].shape);
 b3World* world=b3GetUnlockedWorld(id.world0);if(!world)return 0;
 b3Body_SetType(id,b3_dynamicBody);b3Body_SetMassData(id,mass);
 b3Body* body=b3GetBodyFullId(world,id);b3BodySim* sim=b3GetBodySim(world,body);
 sim->invMass=inverse_mass;
 sim->invInertiaLocal=(b3Matrix3){{inverse_inertia[0],0,0},{0,inverse_inertia[1],0},{0,0,inverse_inertia[2]}};
 b3Matrix3 rotation=b3MakeMatrixFromQuat(sim->transform.q);
 sim->invInertiaWorld=b3MulMM(b3MulMM(rotation,sim->invInertiaLocal),b3Transpose(rotation));
 c->contacts.count=0;return 1;
}
int bw_collision_native_mass_values(void* handle,int index,float* output) {
 BwCollision* c=handle;if(!output || !live_mesh(c,index))return 0;
 b3BodyId id=b3Shape_GetBody(c->meshes[index].shape);b3World* world=b3GetUnlockedWorld(id.world0);if(!world)return 0;
 b3BodySim* sim=b3GetBodySim(world,b3GetBodyFullId(world,id));
 output[0]=sim->invMass;output[1]=sim->invInertiaLocal.cx.x;output[2]=sim->invInertiaLocal.cy.y;output[3]=sim->invInertiaLocal.cz.z;return 1;
}
int bw_collision_angular_impulse(void* handle,int index,const float* impulse,float* output) {
 BwCollision* c=handle;if(!impulse || !output || !live_mesh(c,index))return 0;
 b3Vec3 value={impulse[0],impulse[1],impulse[2]};if(!b3IsValidVec3(value))return 0;
 b3BodyId body=b3Shape_GetBody(c->meshes[index].shape);
 if(!b3Body_IsEnabled(body))return 0; // Never silently discard a detached impulse.
 b3Body_ApplyAngularImpulse(body,value,true);
 b3Vec3 result=b3Body_GetAngularVelocity(body);output[0]=result.x;output[1]=result.y;output[2]=result.z;return 1;
}
// Internal query categories, not native Havok collision-filter values.
static b3QueryFilter solid_query(void) { b3QueryFilter f=b3DefaultQueryFilter();f.maskBits=1;return f; }
typedef int (*BwOverlapSink)(int,void*);
typedef struct {BwCollision* collision;BwOverlapSink sink;void* context;int failed;} BwOverlap;
static bool overlap_candidate(b3ShapeId shape,void* context) {
 BwOverlap* query=context;
 if(query_ignores_shape(query->collision,shape))return true;
 uintptr_t identity=(uintptr_t)b3Shape_GetUserData(shape);
 if(identity==0 || identity>(uintptr_t)query->collision->mesh_count){query->failed=1;return false;}
 if(!query->sink((int)identity-1,query->context)){query->failed=1;return false;}
 return true;
}
// Broadphase candidates only: Box3D may include fat-proxy overlap. No native
// Havok filter or narrowphase is implied. Include erased/non-solid geometry.
int bw_collision_overlap_candidates(void* handle,const float* lower,const float* upper,BwOverlapSink sink,void* context) {
 BwCollision* c=handle;if(!c || !lower || !upper || !sink)return 0;
 b3AABB bounds={{lower[0],lower[1],lower[2]},{upper[0],upper[1],upper[2]}};
 if(!b3IsValidAABB(bounds))return 0;
 b3QueryFilter filter=b3DefaultQueryFilter();filter.maskBits=UINT64_MAX;
 BwOverlap query={c,sink,context,0};
 b3World_OverlapAABB(c->world,bounds,filter,overlap_candidate,&query);
 return !query.failed;
}
// Native AABB phantoms contain collidables (whole compound bodies), not only
// the child shapes that overlap a query. Test actual body-union bounds, without
// fat-proxy padding. Havok's quantization/swept bounds remain compatibility work.
int bw_collision_overlap_body_bounds(void* handle,const float* lower,const float* upper,BwOverlapSink sink,void* context) {
 BwCollision* c=handle;if(!c || !lower || !upper || !sink)return 0;
 b3AABB query={{lower[0],lower[1],lower[2]},{upper[0],upper[1],upper[2]}};
 if(!b3IsValidAABB(query))return 0;
 for(int i=0;i<c->mesh_count;i++) {
  BwStaticMesh* mesh=&c->meshes[i];
  if(mesh->retired || (mesh->group_count>0 && mesh->group_indices[0]!=i) || query_ignores_shape(c,mesh->shape))continue;
  b3BodyId body=b3Shape_GetBody(mesh->shape);if(!b3Body_IsEnabled(body))continue;
  b3AABB bounds=b3Body_ComputeAABB(body);
  if(bounds.lowerBound.x<=query.upperBound.x && bounds.upperBound.x>=query.lowerBound.x &&
     bounds.lowerBound.y<=query.upperBound.y && bounds.upperBound.y>=query.lowerBound.y &&
     bounds.lowerBound.z<=query.upperBound.z && bounds.upperBound.z>=query.lowerBound.z) {
   if(!sink(i,context))return 0;
  }
 }
 return 1;
}
int bw_collision_overlap_sphere(void* handle,const float* center,float radius,BwOverlapSink sink,void* context) {
 BwCollision* c=handle;if(!c || !center || !sink || !isfinite(radius) || radius<0)return 0;
 b3Pos origin={center[0],center[1],center[2]};if(!b3IsValidPosition(origin))return 0;
 b3Vec3 point={0};b3ShapeProxy proxy={&point,1,radius};
 b3QueryFilter filter=b3DefaultQueryFilter();filter.maskBits=UINT64_MAX;
 BwOverlap query={c,sink,context,0};
 b3World_OverlapShape(c->world,origin,&proxy,filter,overlap_candidate,&query);
 return !query.failed;
}
// Geometry stays in its imported frame. All manual queries must use this same
// transform as Box3D's broadphase. This does not set velocity or simulate motion.
int bw_collision_set_transform(void* handle,int index,const float* position,const float* rotation) {
 BwCollision* c=handle;if(!position || !rotation || !live_mesh(c,index))return 0;
 b3Transform t={{position[0],position[1],position[2]},{{rotation[0],rotation[1],rotation[2]},rotation[3]}};
 if(!b3IsValidTransform(t))return 0;
 BwStaticMesh* mesh=&c->meshes[index];
 b3Body_SetTransform(b3Shape_GetBody(mesh->shape),(b3Pos){t.p.x,t.p.y,t.p.z},t.q);
 if(mesh->group_count) {
  for(int i=0;i<mesh->group_count;i++)c->meshes[mesh->group_indices[i]].transform=t;
 } else mesh->transform=t;
 c->contacts.count=0;return 1;
}
float bw_collision_shape_min_y(void* handle,int index) {
 BwCollision* c=handle;if(!live_mesh(c,index))return NAN;
 BwStaticMesh* mesh=&c->meshes[index];
 if(mesh->is_capsule)return b3MinFloat(b3TransformPoint(mesh->transform,mesh->capsule.center1).y,b3TransformPoint(mesh->transform,mesh->capsule.center2).y)-mesh->radius;
 const b3Vec3* points=mesh->hull?b3GetHullPoints(mesh->hull):b3GetMeshVertices(mesh->data);
 int count=mesh->hull?mesh->hull->vertexCount:mesh->data->vertexCount;
 float minimum=FLT_MAX;
 for(int i=0;i<count;i++)minimum=b3MinFloat(minimum,b3TransformPoint(mesh->transform,points[i]).y-mesh->radius);
 return minimum;
}
// Reversible world membership is independent of painted/solid filtering and
// permanent retirement. Disabling removes the body from solver AND broadphase.
int bw_collision_body_enabled(void* handle,int index) {
 BwCollision* c=handle;if(!live_mesh(c,index))return -1;
 return b3Body_IsEnabled(b3Shape_GetBody(c->meshes[index].shape));
}
int bw_collision_set_body_enabled(void* handle,int index,int enabled) {
 BwCollision* c=handle;if(!live_mesh(c,index))return 0;
 b3BodyId body=b3Shape_GetBody(c->meshes[index].shape);
 if(!b3GetUnlockedWorld(body.world0))return 0;
 if(b3Body_IsEnabled(body)==(enabled!=0))return 1;
 BwStaticMesh* record=body_metadata(c,index);
 if(enabled) {
  b3Body_Enable(body);
  if(record->detached) {
   b3Body_SetLinearVelocity(body,record->detached->linear);
   b3Body_SetAngularVelocity(body,record->detached->angular);
   free(record->detached);record->detached=NULL;
  }
 } else {
  if(b3Body_GetType(body)!=b3_staticBody) {
   BwDetachedMotion* saved=malloc(sizeof(BwDetachedMotion));if(!saved)return 0;
   saved->linear=b3Body_GetLinearVelocity(body);saved->angular=b3Body_GetAngularVelocity(body);
   record->detached=saved;
  }
  b3Body_Disable(body);
 }
 c->contacts.count=0;return 1;
}
int bw_collision_retire_shape(void* handle,int index) {
 BwCollision* c=handle;if(!c || index<0 || index>=c->mesh_count)return 0;
 b3Filter filter=b3DefaultFilter();filter.categoryBits=0;filter.maskBits=0;
 b3Shape_SetFilter(c->meshes[index].shape,filter,true);
 c->meshes[index].solid=false;c->meshes[index].retired=true;c->contacts.count=0;return 1;
}
static void release_mesh_payload(BwStaticMesh* mesh) {
 if(mesh->data){b3DestroyMesh(mesh->data);mesh->data=NULL;}
 if(mesh->hull){b3DestroyHull(mesh->hull);mesh->hull=NULL;}
 free(mesh->source_triangles);mesh->source_triangles=NULL;
 free(mesh->detached);mesh->detached=NULL;
 mesh->shape=b3_nullShapeId;mesh->solid=false;mesh->retired=true;
 mesh->group_indices=NULL;mesh->group_count=0;mesh->owns_group=false;
}
// Permanently destroy one imported body and release its Box3D geometry. Rust
// retains stable source indices as tombstones; no caller may use the returned
// native shape again after this succeeds.
int bw_collision_release_shape(void* handle,int index) {
 BwCollision* c=handle;
 if(!c || index<0 || index>=c->mesh_count)return 0;
 BwStaticMesh* mesh=&c->meshes[index];
 if(mesh->retired)return 1;
 b3ShapeId selected=mesh->shape;
 if(!b3Shape_IsValid(selected))return 0;
 int* members=mesh->group_indices;
 int count=mesh->group_count;
 if(count>0) {
  if(!members || count>c->mesh_count)return 0;
  for(int i=0;i<count;i++)if(members[i]<0 || members[i]>=c->mesh_count)return 0;
  bool release_query_ignore=false,release_mass_shape=false;
  for(int i=0;i<count;i++) {
   if(c->query_ignore==members[i])release_query_ignore=true;
   if(c->mass_shape==members[i])release_mass_shape=true;
  }
  b3BodyId body=b3Shape_GetBody(selected);
  if(b3Body_IsValid(body))b3DestroyBody(body);
  for(int i=0;i<count;i++)release_mesh_payload(&c->meshes[members[i]]);
  free(members);
  if(release_query_ignore)c->query_ignore=-1;
  if(release_mass_shape){c->mass_shape=-1;c->mass_policy=NULL;c->mass_context=NULL;
   b3World_SetContactMassCallback(c->world,NULL,NULL);}
 } else {
  if(c->query_ignore==index)c->query_ignore=-1;
  if(c->mass_shape==index){c->mass_shape=-1;c->mass_policy=NULL;c->mass_context=NULL;
   b3World_SetContactMassCallback(c->world,NULL,NULL);}
  b3BodyId body=b3Shape_GetBody(selected);
  if(b3Body_IsValid(body))b3DestroyBody(body);
  release_mesh_payload(mesh);
 }
 c->contacts.count=0;
 return 1;
}
int bw_collision_shape_payload_released(void* handle,int index) {
 BwCollision* c=handle;
 if(!c || index<0 || index>=c->mesh_count)return 0;
 BwStaticMesh* mesh=&c->meshes[index];
 return mesh->retired && !B3_IS_NON_NULL(mesh->shape) && !mesh->data &&
        !mesh->hull && !mesh->source_triangles && !mesh->detached &&
        !mesh->group_indices && mesh->group_count==0;
}
int bw_collision_shape_is_live(void* handle,int index) {
 BwCollision* c=handle;return live_mesh(c,index)?1:0;
}
int bw_collision_set_solid(void* handle,int index,int solid) {
 BwCollision* c=handle;if(index<0 || index>=c->mesh_count)return 0;
 BwStaticMesh* mesh=&c->meshes[index];
 if(mesh->retired || mesh->solid==(solid!=0))return 1;
 b3Filter filter=b3DefaultFilter();filter.categoryBits=solid?1:2;
 b3Shape_SetFilter(mesh->shape,filter,true);mesh->solid=solid!=0;
 c->contacts.count=0;return 1;
}
void* bw_collision_create(void) {
 BwCollision* c=calloc(1,sizeof(BwCollision));if(!c)return NULL;
 c->query_ignore=-1;
 b3WorldDef wd=b3DefaultWorldDef();wd.workerCount=0;wd.gravity=b3Vec3_zero;c->world=b3CreateWorld(&wd);
 if(B3_IS_NULL(c->world)){free(c);return NULL;}
 return c;
}
static int bw_collision_add_mesh_impl(void* handle,const float* points,int triangle_count,float radius,const int* source_ids,int source_start) {
 BwCollision* c=handle;
 if (!c || !points || (!source_ids && source_start<0) || triangle_count<=0 || triangle_count>INT_MAX/3 || c->mesh_count==INT_MAX || source_start>INT_MAX-triangle_count) return 0;
 int n=triangle_count*3; b3Vec3* vertices=malloc((size_t)n*sizeof(b3Vec3));int32_t* indices=malloc((size_t)n*sizeof(int32_t));
 if(!vertices||!indices){free(vertices);free(indices);return 0;}
 for(int i=0;i<n;i++){vertices[i]=(b3Vec3){points[3*i],points[3*i+1],points[3*i+2]};indices[i]=i;}
 b3MeshDef mesh={0};mesh.vertices=vertices;mesh.indices=indices;mesh.vertexCount=n;mesh.triangleCount=triangle_count;mesh.weldVertices=true;mesh.weldTolerance=0.00001f;mesh.identifyEdges=true;
 int* source=malloc((size_t)triangle_count*sizeof(int));
 if(!source){free(vertices);free(indices);return 0;}
 mesh.sourceTriangleIndices=source;
 b3MeshData* geometry=b3CreateMesh(&mesh,NULL,0);free(vertices);free(indices);
 if(!geometry){free(source);return 0;}
 for(int i=0;i<geometry->triangleCount;i++)source[i]=source_ids?source_ids[source[i]]:source_start+source[i];
 BwStaticMesh* grown=realloc(c->meshes,((size_t)c->mesh_count+1)*sizeof(BwStaticMesh));
 if(!grown){b3DestroyMesh(geometry);free(source);return 0;}
 c->meshes=grown;c->meshes[c->mesh_count++]=(BwStaticMesh){geometry,radius,NULL,source};
 b3BodyDef bd=b3DefaultBodyDef();bd.type=b3_staticBody;b3BodyId body=b3CreateBody(c->world,&bd);
 b3ShapeDef sd=b3DefaultShapeDef();sd.convexRadius=radius;sd.enableCustomFiltering=true;sd.userData=(void*)(uintptr_t)c->mesh_count;c->meshes[c->mesh_count-1].shape=b3CreateMeshShape(body,&sd,geometry,(b3Vec3){1,1,1});
 c->meshes[c->mesh_count-1].solid=true;c->meshes[c->mesh_count-1].transform=b3Transform_identity;
 return 1;
}
int bw_collision_add_mesh(void* handle,const float* points,int triangle_count,float radius,const int* source_ids) {
 return bw_collision_add_mesh_impl(handle,points,triangle_count,radius,source_ids,0);
}
int bw_collision_add_mesh_range(void* handle,const float* points,int triangle_count,float radius,int source_start) {
 return bw_collision_add_mesh_impl(handle,points,triangle_count,radius,NULL,source_start);
}
/* A planar convex core plus its original spherical margin is the union of
   rounded triangles covering that core. Retain both sides and source points;
   never invent wall thickness or expand the vertices by the margin. */
static int add_planar_convex(void* handle,const b3Vec3* points,int count,float radius){
 b3Vec3 origin=points[0],axis=b3Vec3_zero,normal=b3Vec3_zero;
 float axis_length=0,normal_length=0;
 for(int i=1;i<count;i++){b3Vec3 v=b3Sub(points[i],origin);float length=b3Dot(v,v);if(length>axis_length){axis=v;axis_length=length;}}
 if(axis_length==0)return 0;
 for(int i=1;i<count;i++){b3Vec3 n=b3Cross(axis,b3Sub(points[i],origin));float length=b3Dot(n,n);if(length>normal_length){normal=n;normal_length=length;}}
 if(normal_length==0)return 0;
 normal=b3Normalize(normal);axis=b3Normalize(axis);
 b3Vec3 second=b3Cross(normal,axis);
 /* Dot/cross arithmetic after a rigid placement can retain float roundoff.
    This gate classifies the plane; it does not project or move any point. */
 float tolerance=16.f*FLT_EPSILON*sqrtf(axis_length);
 b3Point2D* projected=malloc((size_t)count*sizeof(b3Point2D));
 /* Monotone-chain construction temporarily retains both chains. */
 b3Point2D* boundary=malloc((size_t)count*2*sizeof(b3Point2D));
 if(!projected||!boundary){free(projected);free(boundary);return 0;}
 for(int i=0;i<count;i++){
  b3Vec3 v=b3Sub(points[i],origin);
  if(fabsf(b3Dot(normal,v))>tolerance){free(projected);free(boundary);return 0;}
  projected[i]=(b3Point2D){{b3Dot(axis,v),b3Dot(second,v)},0,i};
 }
 int vertices=b3Hull2D(projected,count,boundary);free(projected);
 if(vertices<3){free(boundary);return 0;}
 int triangles=2*(vertices-2);
 b3Vec3* faces=malloc((size_t)triangles*3*sizeof(b3Vec3));int* ids=malloc((size_t)triangles*sizeof(int));
 if(!faces||!ids){free(boundary);free(faces);free(ids);return 0;}
 int triangle=0;
 for(int i=1;i<vertices-1;i++){
  b3Vec3 a=points[boundary[0].originalIndex],b=points[boundary[i].originalIndex],c=points[boundary[i+1].originalIndex];
  faces[triangle*3]=a;faces[triangle*3+1]=b;faces[triangle*3+2]=c;ids[triangle++]=-1;
  faces[triangle*3]=a;faces[triangle*3+1]=c;faces[triangle*3+2]=b;ids[triangle++]=-1;
 }
 free(boundary);int result=bw_collision_add_mesh(handle,(const float*)faces,triangles,radius,ids);
 free(faces);free(ids);return result?2:0;
}
int bw_collision_hull_capacity(void){return B3_MAX_HULL_VERTICES;}
int bw_collision_hull_vertex_count(void* handle,int index){
 BwCollision* c=handle;if(!live_mesh(c,index) || !c->meshes[index].hull)return -1;
 return c->meshes[index].hull->vertexCount;
}
int bw_collision_add_convex(void* handle,const float* points,int count,float radius){
 BwCollision* c=handle;
 if(!points || count<4 || count>B3_MAX_HULL_VERTICES || c->mesh_count==INT_MAX)return 0;
 b3HullData* hull=b3CreateHull((const b3Vec3*)points,count,count);if(!hull)return add_planar_convex(handle,(const b3Vec3*)points,count,radius);
 BwStaticMesh* grown=realloc(c->meshes,((size_t)c->mesh_count+1)*sizeof(BwStaticMesh));
 if(!grown){b3DestroyHull(hull);return 0;}
 c->meshes=grown;c->meshes[c->mesh_count++]=(BwStaticMesh){NULL,radius,hull};
 b3BodyDef bd=b3DefaultBodyDef();bd.type=b3_staticBody;b3BodyId body=b3CreateBody(c->world,&bd);
 b3ShapeDef sd=b3DefaultShapeDef();sd.convexRadius=radius;sd.enableCustomFiltering=true;sd.userData=(void*)(uintptr_t)c->mesh_count;c->meshes[c->mesh_count-1].shape=b3CreateHullShape(body,&sd,hull);
 c->meshes[c->mesh_count-1].solid=true;c->meshes[c->mesh_count-1].transform=b3Transform_identity;
 return 1;
}
int bw_collision_hull_edges(void* handle,int index) {
 BwCollision* c=handle;if(!live_mesh(c,index) || !c->meshes[index].hull)return -1;
 return c->meshes[index].hull->edgeCount;
}
int bw_collision_add_capsule(void* handle,const float* endpoints,float radius){
 BwCollision* c=handle;if(!c || !endpoints || !isfinite(radius) || radius<=0 || c->mesh_count==INT_MAX)return 0;
 for(int i=0;i<6;i++)if(!isfinite(endpoints[i]))return 0;
 b3Capsule capsule={{endpoints[0],endpoints[1],endpoints[2]},{endpoints[3],endpoints[4],endpoints[5]},radius};
 BwStaticMesh* grown=realloc(c->meshes,((size_t)c->mesh_count+1)*sizeof(BwStaticMesh));if(!grown)return 0;
 c->meshes=grown;
 b3BodyDef bd=b3DefaultBodyDef();bd.type=b3_staticBody;b3BodyId body=b3CreateBody(c->world,&bd);if(B3_IS_NULL(body))return 0;
 b3ShapeDef sd=b3DefaultShapeDef();sd.enableCustomFiltering=true;sd.userData=(void*)(uintptr_t)(c->mesh_count+1);
 b3ShapeId shape=b3CreateCapsuleShape(body,&sd,&capsule);if(B3_IS_NULL(shape)){b3DestroyBody(body);return 0;}
 c->meshes[c->mesh_count++]=(BwStaticMesh){.radius=radius,.shape=shape,.solid=true,.transform=b3Transform_identity,.capsule=capsule,.is_capsule=true};
 return 1;
}
static b3CastOutput cast_static(BwStaticMesh* shape,const b3ShapeCastInput* input){
 b3Vec3 points[B3_MAX_HULL_VERTICES];
 if(input->proxy.count>B3_MAX_HULL_VERTICES){fputs("Blotworx cast proxy exceeds Box3D hull limit\n",stderr);abort();}
 for(int i=0;i<input->proxy.count;i++)points[i]=b3InvTransformPoint(shape->transform,input->proxy.points[i]);
 b3ShapeCastInput local=*input;local.proxy.points=points;
 local.translation=b3InvRotateVector(shape->transform.q,input->translation);
 b3Mesh mesh={shape->data,{1,1,1}};
 // Callers combine the target radius with the moving proxy. Use a zero-radius
 // segment here so an analytic capsule does not receive that radius twice.
 b3Capsule core=shape->capsule;core.radius=0;
 b3CastOutput hit=shape->is_capsule?b3ShapeCastCapsule(&core,&local):shape->hull?b3ShapeCastHull(shape->hull,&local):b3ShapeCastMesh(&mesh,&local);
 if(hit.hit){hit.point=b3TransformPoint(shape->transform,hit.point);hit.normal=b3RotateVector(shape->transform.q,hit.normal);}
 return hit;
}
void bw_collision_destroy(void* handle){BwCollision* c=handle;if(c){b3DestroyWorld(c->world);for(int i=0;i<c->mesh_count;i++){if(c->meshes[i].data)b3DestroyMesh(c->meshes[i].data);if(c->meshes[i].hull)b3DestroyHull(c->meshes[i].hull);free(c->meshes[i].source_triangles);free(c->meshes[i].detached);if(c->meshes[i].owns_group)free(c->meshes[i].group_indices);}free(c->meshes);if(c->mover)b3DestroyHull(c->mover);free(c->contacts.planes);free(c->contacts.points);free(c->contacts.mesh_indices);free(c->contacts.source_triangles);free(c->contacts.simplexes);free(c->contacts.saved_offsets);free(c->touch_contacts);free(c);}}
int bw_collision_set_hull(void* handle,const float* points,int count,float margin){
 BwCollision* c=handle;
 if(!points || count<4 || count>B3_MAX_HULL_VERTICES || margin<0)return 0;
 b3HullData* hull=b3CreateHull((const b3Vec3*)points,count,count);
 if(!hull)return 0;
 if(c->mover)b3DestroyHull(c->mover);
 c->mover=hull;c->margin=margin;return 1;
}
int bw_collision_contact_count(void* handle){return ((BwCollision*)handle)->contacts.count;}
int bw_collision_contact(void* handle,int index,float* output){
 BwCollision* c=handle;if(index<0 || index>=c->contacts.count || !output)return 0;
 b3Plane p=c->contacts.planes[index].plane;
 output[0]=p.normal.x;output[1]=p.normal.y;output[2]=p.normal.z;output[3]=p.offset;return 1;
}
typedef struct {BwCollision* collision;b3RayResult hit;} BwClosestQuery;
static b3CastResultFcn closest_query_hit;
static float closest_query_hit(b3ShapeId shape,b3Pos point,b3Vec3 normal,float fraction,uint64_t material,int triangle,int child,void* context) {
 BwClosestQuery* q=context;if(query_ignores_shape(q->collision,shape))return -1.0f;
 if(!q->hit.hit || fraction<q->hit.fraction){
  q->hit.shapeId=shape;q->hit.point=point;q->hit.normal=normal;
  q->hit.fraction=fraction;q->hit.userMaterialId=material;q->hit.triangleIndex=triangle;q->hit.childIndex=child;q->hit.hit=true;
 }
 return q->hit.fraction;
}
static b3RayResult closest_query_ray(BwCollision* c,b3Pos origin,b3Vec3 delta) {
 if(c->query_ignore<0)return b3World_CastRayClosest(c->world,origin,delta,solid_query());
 BwClosestQuery q={.collision=c,.hit={.fraction=1.0f}};
 b3World_CastRay(c->world,origin,delta,solid_query(),closest_query_hit,&q);return q.hit;
}
float bw_collision_ray(void* handle,const float* origin,const float* direction,float distance){
 BwCollision* c=handle;b3RayResult hit=closest_query_ray(c,(b3Pos){origin[0],origin[1],origin[2]},(b3Vec3){direction[0]*distance,direction[1]*distance,direction[2]*distance});return hit.hit?hit.fraction*distance:-1.f;
}
int bw_collision_ray_hit(void* handle,const float* origin,const float* direction,float distance,float* output){
 BwCollision* c=handle;
 b3RayResult hit=closest_query_ray(c,(b3Pos){origin[0],origin[1],origin[2]},(b3Vec3){direction[0]*distance,direction[1]*distance,direction[2]*distance});
 if(!hit.hit)return -1;
 output[0]=hit.fraction*distance;
 output[1]=(float)hit.point.x;output[2]=(float)hit.point.y;output[3]=(float)hit.point.z;
 output[4]=hit.normal.x;output[5]=hit.normal.y;output[6]=hit.normal.z;
 return (int)(uintptr_t)b3Shape_GetUserData(hit.shapeId)-1;
}
typedef struct {int mesh,triangle,material,source_triangle;float fraction;float point[3],normal[3];} BwRayHit;
typedef int (*BwRaySink)(const BwRayHit*,void*);
typedef struct {const b3MeshData* mesh;const b3RayCastInput* input;int index;const int* source;BwRaySink sink;void* context;b3Transform transform;} BwMeshRay;
static int emit_ray_hit(int mesh,int triangle,int material,int source_triangle,const b3CastOutput* hit,BwRaySink sink,void* context){
 BwRayHit value={mesh,triangle,material,source_triangle,hit->fraction,
  {hit->point.x,hit->point.y,hit->point.z},{hit->normal.x,hit->normal.y,hit->normal.z}};
 return sink(&value,context);
}
// Same Box3D BVH overlap and triangle intersection primitives as b3RayCastMesh,
// but never shorten the ray after a hit. Native triangle indices are retained;
// these are Box3D indices, NOT Havok shape keys.
static int ray_mesh_node(const b3MeshNode* node,const BwMeshRay* query){
 b3V32 start=b3LoadV(&query->input->origin.x),delta=b3LoadV(&query->input->translation.x);
 b3V32 end=b3AddV(start,delta),lo=b3LoadV(&node->lowerBound.x),hi=b3LoadV(&node->upperBound.x);
 if(!b3TestBoundsOverlap(lo,hi,b3MinV(start,end),b3MaxV(start,end)) || !b3TestBoundsRayOverlap(lo,hi,start,delta))return 1;
 if(node->data.asLeaf.type!=3){
  return ray_mesh_node(node+1,query) && ray_mesh_node(node+node->data.asNode.childOffset,query);
 }
 const b3MeshTriangle* triangles=b3GetMeshTriangles(query->mesh);
 const b3Vec3* vertices=b3GetMeshVertices(query->mesh);
 const uint8_t* materials=b3GetMeshMaterialIndices(query->mesh);
 for(int i=0;i<node->data.asLeaf.triangleCount;i++){
  int index=node->triangleOffset+i;b3MeshTriangle tri=triangles[index];
  b3Vec3 a=vertices[tri.index1],b=vertices[tri.index2],c=vertices[tri.index3];
  float fraction=b3IntersectRayTriangle(start,delta,b3LoadV(&a.x),b3LoadV(&b.x),b3LoadV(&c.x));
  if(fraction<1.f){
   b3CastOutput hit={0};hit.fraction=fraction;
   hit.point=b3Add(query->input->origin,b3MulSV(fraction,query->input->translation));
   hit.normal=b3Normalize(b3Cross(b3Sub(b,a),b3Sub(c,a)));
   hit.point=b3TransformPoint(query->transform,hit.point);
   hit.normal=b3RotateVector(query->transform.q,hit.normal);
   if(!emit_ray_hit(query->index,index,materials[index],query->source[index],&hit,query->sink,query->context))return 0;
  }
 }
 return 1;
}
int bw_collision_ray_all(void* handle,const float* origin,const float* translation,BwRaySink sink,void* context){
 BwCollision* c=handle;b3RayCastInput input={{origin[0],origin[1],origin[2]},
  {translation[0],translation[1],translation[2]},1.f};
 for(int i=0;i<c->mesh_count;i++){
  if(query_ignores_index(c,i))continue;
  BwStaticMesh* mesh=&c->meshes[i];
  b3RayCastInput local=input;local.origin=b3InvTransformPoint(mesh->transform,input.origin);
  local.translation=b3InvRotateVector(mesh->transform.q,input.translation);
  if(mesh->hull || mesh->is_capsule){
   b3CastOutput hit=mesh->is_capsule?b3RayCastCapsule(&mesh->capsule,&local):b3RayCastHull(mesh->hull,&local);
   if(hit.hit){hit.point=b3TransformPoint(mesh->transform,hit.point);hit.normal=b3RotateVector(mesh->transform.q,hit.normal);}
   if(hit.hit && !emit_ray_hit(i,-1,-1,-1,&hit,sink,context))return 0;
  }else{
   BwMeshRay query={mesh->data,&local,i,mesh->source_triangles,sink,context,mesh->transform};
   if(!ray_mesh_node(b3GetMeshNodes(mesh->data),&query))return 0;
  }
 }
 return 1;
}
int bw_collision_sweep_sphere(void* handle,const float* origin,const float* translation,float radius,float* output,int* source_triangle){
 BwCollision* c=handle;b3Vec3 center={origin[0],origin[1],origin[2]};
 b3ShapeCastInput cast={0};cast.proxy=(b3ShapeProxy){&center,1,radius};
 cast.translation=(b3Vec3){translation[0],translation[1],translation[2]};cast.maxFraction=1;
 int owner=-1;
 for(int i=0;i<c->mesh_count;i++){
  if(query_ignores_index(c,i))continue;
  b3Mesh mesh={c->meshes[i].data,{1,1,1}};
  cast.proxy.radius=radius+c->meshes[i].radius;
  b3CastOutput hit=cast_static(&c->meshes[i],&cast);
  if(hit.hit && (owner<0 || hit.fraction<cast.maxFraction)){
   owner=i;cast.maxFraction=hit.fraction;
   *source_triangle=c->meshes[i].hull || c->meshes[i].is_capsule || hit.triangleIndex<0?-1:c->meshes[i].source_triangles[hit.triangleIndex];
   output[0]=hit.fraction;
   output[1]=hit.point.x;output[2]=hit.point.y;output[3]=hit.point.z;
   output[4]=hit.normal.x;output[5]=hit.normal.y;output[6]=hit.normal.z;
  }
 }
 return owner;
}
typedef int (*BwSphereFilter)(int mesh,int source_triangle,void* context);
typedef struct {BwStaticMesh* mesh;int index;b3Vec3 center,delta;float radius;BwRaySink sink;void* context;int failed;BwSphereFilter filter;} BwSphereAll;
static bool sphere_all_triangle(b3Vec3 a,b3Vec3 b,b3Vec3 d,int index,void* context){
 BwSphereAll* q=context;
 if(q->filter){int admitted=q->filter(q->index,q->mesh->source_triangles[index],q->context);
  if(admitted<0){q->failed=1;return false;}if(!admitted)return true;}
 // Same one-sided start-center test as Box3D b3ShapeCastMesh.
 if(b3Dot(b3Cross(b3Sub(b,a),b3Sub(d,a)),b3Sub(q->center,a))<0)return true;
 b3Vec3 triangle[3]={b3Vec3_zero,b3Sub(b,a),b3Sub(d,a)};
 b3ShapeCastPairInput input={0};input.proxyA=(b3ShapeProxy){triangle,3,0};
 input.proxyB=(b3ShapeProxy){&q->center,1,q->radius};
 input.transform=(b3Transform){b3Neg(a),b3Quat_identity};
 input.translationB=q->delta;input.maxFraction=1;
 b3CastOutput hit=b3ShapeCast(&input);
 if(hit.hit){
  hit.point=b3TransformPoint(q->mesh->transform,b3Add(hit.point,a));
  hit.normal=b3RotateVector(q->mesh->transform.q,hit.normal);
  if(!emit_ray_hit(q->index,index,b3GetMeshMaterialIndices(q->mesh->data)[index],
      q->mesh->source_triangles[index],&hit,q->sink,q->context)){q->failed=1;return false;}
 }
 return true;
}
// Real Box3D all-hit sweep, without closest-fraction clipping. Native Havok
// filters, cast tolerances, keys and broadphase delivery order remain separate.
int bw_collision_sphere_filtered(void* handle,const float* origin,const float* translation,float radius,BwRaySink sink,BwSphereFilter filter,void* context){
 BwCollision* c=handle;if(!c || !origin || !translation || !sink || !isfinite(radius) || radius<0)return 0;
 b3Vec3 center={origin[0],origin[1],origin[2]},delta={translation[0],translation[1],translation[2]};
 for(int i=0;i<c->mesh_count;i++){
  if(query_ignores_index(c,i))continue;
  BwStaticMesh* mesh=&c->meshes[i];float combined=radius+mesh->radius;if(!isfinite(combined))return 0;
  if(mesh->hull || mesh->is_capsule){
   if(filter){int admitted=filter(i,-1,context);if(admitted<0)return 0;if(!admitted)continue;}
   b3ShapeCastInput input={0};input.proxy=(b3ShapeProxy){&center,1,combined};input.translation=delta;input.maxFraction=1;
   b3CastOutput hit=cast_static(mesh,&input);
   if(hit.hit && !emit_ray_hit(i,-1,-1,-1,&hit,sink,context))return 0;
  }else{
   BwSphereAll q={mesh,i,b3InvTransformPoint(mesh->transform,center),b3InvRotateVector(mesh->transform.q,delta),combined,sink,context,0,filter};
   b3Vec3 end=b3Add(q.center,q.delta),extent={combined,combined,combined};
   b3AABB bounds={b3Sub(b3Min(q.center,end),extent),b3Add(b3Max(q.center,end),extent)};
   if(!b3IsValidAABB(bounds))return 0;
   b3Mesh view={mesh->data,{1,1,1}};b3QueryMesh(&view,bounds,sphere_all_triangle,&q);
   if(q.failed)return 0;
  }
 }
 return 1;
}
int bw_collision_sphere_all(void* handle,const float* origin,const float* translation,float radius,BwRaySink sink,void* context){
 return bw_collision_sphere_filtered(handle,origin,translation,radius,sink,NULL,context);
}

// Per-child overlap identities for camera placement and other material-aware
// queries. A shape-only overlap loses which painted triangle was touched.
typedef struct {BwStaticMesh* mesh;int index;b3Vec3 center;float radius;BwSphereFilter sink;void* context;int failed;} BwSphereOverlapAll;
static bool overlap_all_triangle(b3Vec3 a,b3Vec3 b,b3Vec3 d,int index,void* context){
 BwSphereOverlapAll* q=context;b3Vec3 triangle[3]={a,b,d};
 b3DistanceInput input={0};input.proxyA=(b3ShapeProxy){triangle,3,0};
 input.proxyB=(b3ShapeProxy){&q->center,1,q->radius};input.transform=b3Transform_identity;input.useRadii=true;
 b3SimplexCache cache={0};b3DistanceOutput result=b3ShapeDistance(&input,&cache,NULL,0);
 if(result.distance<B3_OVERLAP_SLOP && q->sink(q->index,q->mesh->source_triangles[index],q->context)<=0){q->failed=1;return false;}
 return true;
}
int bw_collision_overlap_sphere_all(void* handle,const float* center,float radius,BwSphereFilter sink,void* context){
 BwCollision* c=handle;if(!c || !center || !sink || !isfinite(radius) || radius<0)return 0;
 b3Vec3 world_center={center[0],center[1],center[2]};if(!b3IsValidVec3(world_center))return 0;
 for(int i=0;i<c->mesh_count;i++){
  if(query_ignores_index(c,i))continue;
  BwStaticMesh* mesh=&c->meshes[i];float combined=radius+mesh->radius;if(!isfinite(combined))return 0;
  b3Vec3 local=b3InvTransformPoint(mesh->transform,world_center);
  if(mesh->hull || mesh->is_capsule){
   b3DistanceInput input={0};input.proxyA=mesh->is_capsule?(b3ShapeProxy){&mesh->capsule.center1,2,0}:
    (b3ShapeProxy){b3GetHullPoints(mesh->hull),mesh->hull->vertexCount,0};
   input.proxyB=(b3ShapeProxy){&local,1,combined};input.transform=b3Transform_identity;input.useRadii=true;
   b3SimplexCache cache={0};b3DistanceOutput result=b3ShapeDistance(&input,&cache,NULL,0);
   if(result.distance<B3_OVERLAP_SLOP && sink(i,-1,context)<=0)return 0;
  }else{
   BwSphereOverlapAll q={mesh,i,local,combined,sink,context,0};b3Vec3 extent={combined,combined,combined};
   b3AABB bounds={b3Sub(local,extent),b3Add(local,extent)};if(!b3IsValidAABB(bounds))return 0;
   b3Mesh view={mesh->data,{1,1,1}};b3QueryMesh(&view,bounds,overlap_all_triangle,&q);if(q.failed)return 0;
  }
 }
 return 1;
}
static bool planes_with_mesh(b3ShapeId shape,const b3PlaneResult* results,int count,void* context,int mesh_index,int source_triangle){
 (void)shape;BwPlanes* p=context;
 if(count<0 || count>INT_MAX-p->count){fputs("Blotworx collision contact count overflow\n",stderr);abort();}
 int required=p->count+count;
 if(required>p->capacity){
  int capacity=p->capacity && p->capacity<=INT_MAX/2?p->capacity*2:required;
  if(capacity<required)capacity=required;
  if((size_t)capacity>SIZE_MAX/sizeof(b3CollisionPlane)){fputs("Blotworx collision allocation overflow\n",stderr);abort();}
  b3CollisionPlane* grown=realloc(p->planes,(size_t)capacity*sizeof(b3CollisionPlane));
  if(!grown){fputs("Blotworx collision contact allocation failed\n",stderr);abort();}
  p->planes=grown;
  b3Vec3* points=realloc(p->points,(size_t)capacity*sizeof(b3Vec3));
  if(!points){fputs("Blotworx contact point allocation failed\n",stderr);abort();}
  p->points=points;
  int* indices=realloc(p->mesh_indices,(size_t)capacity*sizeof(int));
  if(!indices){fputs("Blotworx contact ownership allocation failed\n",stderr);abort();}
  p->mesh_indices=indices;
  int* source_triangles=realloc(p->source_triangles,(size_t)capacity*sizeof(int));
  if(!source_triangles){fputs("Blotworx contact triangle allocation failed\n",stderr);abort();}
  p->source_triangles=source_triangles;
  b3SimplexCache* simplexes=realloc(p->simplexes,(size_t)capacity*sizeof(b3SimplexCache));
  if(!simplexes){fputs("Blotworx contact simplex allocation failed\n",stderr);abort();}
  p->simplexes=simplexes;
  float* offsets=realloc(p->saved_offsets,(size_t)capacity*sizeof(float));
  if(!offsets){fputs("Blotworx contact offset allocation failed\n",stderr);abort();}
  p->saved_offsets=offsets;p->capacity=capacity;
 }
 for(int i=0;i<count;i++){p->simplexes[p->count]=(b3SimplexCache){0};p->points[p->count]=results[i].point;p->mesh_indices[p->count]=mesh_index;p->source_triangles[p->count]=source_triangle;p->planes[p->count++]=(b3CollisionPlane){.plane=results[i].plane,.pushLimit=FLT_MAX,.push=0,.clipVelocity=true};}return true;
}
typedef struct {BwCollision* collision;BwPlanes* planes;} BwCapsulePlanes;
static bool planes(b3ShapeId shape,const b3PlaneResult* results,int count,void* context){
 BwCapsulePlanes* q=context;if(query_ignores_shape(q->collision,shape))return true;
 return planes_with_mesh(shape,results,count,q->planes,-1,-1);
}
static bool mover_query_filter(b3ShapeId shape,void* context) {
 return !query_ignores_shape(context,shape);
}
int bw_collision_contact_point(void* handle,int index,float* output){
 BwCollision* c=handle;if(index<0 || index>=c->contacts.count || !output)return 0;
 b3Vec3 p=c->contacts.points[index];output[0]=p.x;output[1]=p.y;output[2]=p.z;return 1;
}
int bw_collision_contact_mesh(void* handle,int index){
 BwCollision* c=handle;return index>=0 && index<c->contacts.count?c->contacts.mesh_indices[index]:-1;
}
int bw_collision_contact_source_triangle(void* handle,int index){
 BwCollision* c=handle;return index>=0 && index<c->contacts.count?c->contacts.source_triangles[index]:-1;
}

// Raw Box3D proxy IDs, not Havok feature IDs. Zero denotes no GJK witness.
int bw_collision_contact_simplex(void* handle,int index,uint8_t* output){
 BwCollision* c=handle;if(index<0 || index>=c->contacts.count || !output)return 0;
 const b3SimplexCache* cache=&c->contacts.simplexes[index];
 for(int i=0;i<cache->count;i++){output[2*i]=cache->indexA[i];output[2*i+1]=cache->indexB[i];}
 return cache->count;
}

typedef int (*BwPenetrationSink)(int source_triangle,float depth,void* context);
typedef struct { BwCollision* collision; b3Vec3 position; BwStaticMesh* mesh; float margin; float query_distance; BwPenetrationSink sink; void* sink_context; } BwHullQuery;
static bool hull_triangle(b3Vec3 a,b3Vec3 b,b3Vec3 d,int index,void* context){
 BwHullQuery* q=context;BwCollision* c=q->collision;
 if(c->character_contact && !q->sink) {
  b3Transform t=q->mesh->transform;
  float position[3]={q->position.x,q->position.y,q->position.z};
  float delta[7]={t.p.x,t.p.y,t.p.z,t.q.v.x,t.q.v.y,t.q.v.z,t.q.s},output[7];
  int result=c->character_contact((int)(q->mesh-c->meshes),q->mesh->source_triangles[index],
    position,delta,q->query_distance,output,c->character_context);
  if(result<0){c->character_failed=1;return false;}
  if(result==0)return true;
  if(result==1) {
   b3PlaneResult plane={0};plane.point=(b3Vec3){output[0],output[1],output[2]};
   plane.plane=(b3Plane){{output[3],output[4],output[5]},-output[6]};
   planes_with_mesh(b3_nullShapeId,&plane,1,&c->contacts,(int)(q->mesh-c->meshes),q->mesh->source_triangles[index]);
   return true;
  }
 }
 a=b3Sub(b3TransformPoint(q->mesh->transform,a),q->position);
 b=b3Sub(b3TransformPoint(q->mesh->transform,b),q->position);
 d=b3Sub(b3TransformPoint(q->mesh->transform,d),q->position);
 if(b3Dot(b3Cross(b3Sub(b,a),b3Sub(d,a)),b3Sub(c->mover->center,a))<0)return true;
 b3Vec3 triangle[3]={a,b,d};
 b3DistanceInput input={0};input.proxyA=(b3ShapeProxy){triangle,3,0};
 input.proxyB=(b3ShapeProxy){b3GetHullPoints(c->mover),c->mover->vertexCount,0};
 input.transform=b3Transform_identity;
 b3SimplexCache cache={0};b3DistanceOutput distance=b3ShapeDistance(&input,&cache,NULL,0);
 b3PlaneResult result={0};
 if(distance.distance>0){
  if(distance.distance>q->margin+q->query_distance)return true;
  result.plane=(b3Plane){distance.normal,q->margin-distance.distance};
  result.point=distance.pointA;
 }else{
  // GJK has no penetration normal. Recover intersecting cores using SAT.
  // Triangle clipping can add at most three vertices to a convex hull face.
  b3LocalManifoldPoint points[B3_MAX_HULL_VERTICES+3];
  b3LocalManifold manifold={0};manifold.points=points;b3SATCache sat={0};
  const uint8_t* flags=b3GetMeshFlags(q->mesh->data);
  b3CollideTriangleAndHull(&manifold,B3_MAX_HULL_VERTICES+3,a,b,d,flags?flags[index]:0,c->mover,&sat,false);
  if(manifold.pointCount==0)return true;
  float separation=FLT_MAX;
  for(int i=0;i<manifold.pointCount;i++)if(points[i].separation<separation){separation=points[i].separation;result.point=points[i].point;}
  result.plane=(b3Plane){manifold.normal,q->margin-separation};
 }
 result.point=b3Add(result.point,q->position);
 if(q->sink)return q->sink(q->mesh->source_triangles[index],result.plane.offset,q->sink_context)!=0;
 planes_with_mesh(b3_nullShapeId,&result,1,&c->contacts,(int)(q->mesh-c->meshes),q->mesh->source_triangles[index]);
 if(distance.distance>0)c->contacts.simplexes[c->contacts.count-1]=cache;
 return true;
}
static void convex_contacts(BwCollision* c,int index,b3Vec3 position,float query_distance){
 BwStaticMesh* shape=&c->meshes[index];float margin=c->margin+shape->radius;
 b3Transform transform=b3Transform_identity;transform.p=position;
 transform=b3InvMulTransforms(shape->transform,transform);
 b3DistanceInput input={0};
 input.proxyA=shape->is_capsule?(b3ShapeProxy){&shape->capsule.center1,2,0}:(b3ShapeProxy){b3GetHullPoints(shape->hull),shape->hull->vertexCount,0};
 input.proxyB=(b3ShapeProxy){b3GetHullPoints(c->mover),c->mover->vertexCount,0};
 input.transform=transform;
 b3SimplexCache cache={0};b3DistanceOutput distance=b3ShapeDistance(&input,&cache,NULL,0);
 b3PlaneResult result={0};
 if(distance.distance>0){
  if(distance.distance>margin+query_distance)return;
  result.plane=(b3Plane){distance.normal,margin-distance.distance};
  result.point=distance.pointA;
 }else{
  b3LocalManifoldPoint points[2*B3_MAX_HULL_VERTICES];
  b3LocalManifold manifold={0};manifold.points=points;b3SATCache sat={0};
  if(shape->is_capsule){
   b3Capsule core=shape->capsule;core.radius=0;
   b3SimplexCache simplex={0};
   b3CollideHullAndCapsule(&manifold,2*B3_MAX_HULL_VERTICES,c->mover,&core,b3InvertTransform(transform),&simplex);
   // Box3D supplies this manifold in the moving hull's frame, with the
   // opposite normal. Publish it in the target frame like the hull path.
   manifold.normal=b3Neg(b3RotateVector(transform.q,manifold.normal));
   for(int j=0;j<manifold.pointCount;j++)points[j].point=b3TransformPoint(transform,points[j].point);
  }else b3CollideHulls(&manifold,2*B3_MAX_HULL_VERTICES,shape->hull,c->mover,transform,&sat);
  if(!manifold.pointCount)return;
  float separation=FLT_MAX;
  for(int j=0;j<manifold.pointCount;j++)if(points[j].separation<separation){separation=points[j].separation;result.point=points[j].point;}
  result.plane=(b3Plane){manifold.normal,margin-separation};
 }
 result.point=b3TransformPoint(shape->transform,result.point);
 result.plane.normal=b3RotateVector(shape->transform.q,result.plane.normal);
 planes_with_mesh(b3_nullShapeId,&result,1,&c->contacts,index,-1);
}
static void hull_contacts_with_distance(BwCollision* c,b3Vec3 p,float query_distance){
 c->contacts.count=0;
 for(int i=0;i<c->mesh_count;i++){
  if(!c->meshes[i].solid || query_ignores_index(c,i))continue;
  if(c->meshes[i].hull || c->meshes[i].is_capsule){convex_contacts(c,i,p,query_distance);continue;}
  float margin=c->margin+c->meshes[i].radius;
  b3AABB bounds=c->mover->aabb;
  b3Vec3 extent={margin+query_distance,margin+query_distance,margin+query_distance};
  bounds.lowerBound=b3Sub(b3Add(bounds.lowerBound,p),extent);
  bounds.upperBound=b3Add(b3Add(bounds.upperBound,p),extent);
  b3Mesh mesh={c->meshes[i].data,{1,1,1}};BwHullQuery query={c,p,&c->meshes[i],margin,query_distance};
  bounds=b3AABB_Transform(b3InvertTransform(c->meshes[i].transform),bounds);
  b3QueryMesh(&mesh,bounds,hull_triangle,&query);
 }
}
static void hull_contacts(BwCollision* c,b3Vec3 p){hull_contacts_with_distance(c,p,0.001f);}
int bw_collision_shape_penetrations(void* handle,int shape,const float* position,BwPenetrationSink sink,void* context){
 BwCollision* original=handle;
 if(!original || !original->mover || !sink || shape<0 || shape>=original->mesh_count)return 0;
 if(query_ignores_index(original,shape))return 1;
 BwCollision query=*original;query.contacts=(BwPlanes){0};
 b3Vec3 p={position[0],position[1],position[2]};
 if(query.meshes[shape].hull || query.meshes[shape].is_capsule){
  convex_contacts(&query,shape,p,0);
  for(int i=0;i<query.contacts.count;i++)if(!sink(-1,query.contacts.planes[i].plane.offset,context))break;
  free(query.contacts.planes);free(query.contacts.points);free(query.contacts.mesh_indices);free(query.contacts.source_triangles);free(query.contacts.simplexes);free(query.contacts.saved_offsets);
 }else{
  float margin=query.margin+query.meshes[shape].radius;
  b3Vec3 extent={margin,margin,margin};b3AABB bounds=query.mover->aabb;
  bounds.lowerBound=b3Sub(b3Add(bounds.lowerBound,p),extent);
  bounds.upperBound=b3Add(b3Add(bounds.upperBound,p),extent);
  b3Mesh mesh={query.meshes[shape].data,{1,1,1}};
  BwHullQuery q={&query,p,&query.meshes[shape],margin,0,sink,context};
  bounds=b3AABB_Transform(b3InvertTransform(query.meshes[shape].transform),bounds);
  b3QueryMesh(&mesh,bounds,hull_triangle,&q);
 }
 return 1;
}
// Read-only narrowphase query, including erased geometry. Scratch contact
// storage must never replace the controller's persisted solver contacts.
int bw_collision_shape_penetration(void* handle,int shape,const float* position,float* depth){
 BwCollision* original=handle;
 if(!original || !original->mover || shape<0 || shape>=original->mesh_count)return 0;
 if(query_ignores_index(original,shape)){*depth=0;return 1;}
 BwCollision query=*original;query.contacts=(BwPlanes){0};
 b3Vec3 p={position[0],position[1],position[2]};
 if(query.meshes[shape].hull || query.meshes[shape].is_capsule)convex_contacts(&query,shape,p,0);
 else{
  float margin=query.margin+query.meshes[shape].radius;
  b3Vec3 extent={margin,margin,margin};b3AABB bounds=query.mover->aabb;
  bounds.lowerBound=b3Sub(b3Add(bounds.lowerBound,p),extent);
  bounds.upperBound=b3Add(b3Add(bounds.upperBound,p),extent);
  b3Mesh mesh={query.meshes[shape].data,{1,1,1}};
  BwHullQuery context={&query,p,&query.meshes[shape],margin,0};
  bounds=b3AABB_Transform(b3InvertTransform(query.meshes[shape].transform),bounds);
  b3QueryMesh(&mesh,bounds,hull_triangle,&context);
 }
 *depth=0;
 for(int i=0;i<query.contacts.count;i++)*depth=b3MaxFloat(*depth,query.contacts.planes[i].plane.offset);
 free(query.contacts.planes);free(query.contacts.points);free(query.contacts.mesh_indices);free(query.contacts.source_triangles);free(query.contacts.simplexes);free(query.contacts.saved_offsets);
 return 1;
}
// Box3D's mover adds LINEAR_SLOP to separation, admitting that much
// penetration. Cancel this solver allowance without changing authored radii
// or persisted contact distances. Convergence remains Box3D's own policy.
static b3PlaneSolverResult solve_character_planes(b3Vec3 delta,BwPlanes* contacts){
 for(int i=0;i<contacts->count;i++){
  contacts->saved_offsets[i]=contacts->planes[i].plane.offset;
  contacts->planes[i].plane.offset=contacts->saved_offsets[i]+B3_LINEAR_SLOP;
 }
 b3PlaneSolverResult result=b3SolvePlanes(delta,contacts->planes,contacts->count);
 for(int i=0;i<contacts->count;i++)contacts->planes[i].plane.offset=contacts->saved_offsets[i];
 return result;
}
// Compatibility support probe. Keep geometric radii unchanged: query expansion
// collects nearby planes with their actual signed separation, not inflated hulls.
int bw_collision_support(void* handle,const float* position,const float* requested,float dt,float* solved){
 BwCollision* c=handle;if(!c->mover)return -1;
 b3Vec3 p={position[0],position[1],position[2]};
 b3Vec3 delta={dt*requested[0],dt*requested[1],dt*requested[2]};
 hull_contacts_with_distance(c,p,b3Length(delta));
 if(c->character_failed)return -2;
 solve_character_planes(delta,&c->contacts);
 // Havok's classifier consumes the simplex output VELOCITY (+10), not
 // displacement divided by dt (REL23a7e8 / DOL802f60d0). A probe can cross
 // a small gap before stopping on a flat floor; its final velocity is zero,
 // while its average displacement is nonzero. Feeding the average labels
 // ordinary support as sliding and repeatedly activates slope gravity.
 b3Vec3 velocity=b3ClipVector((b3Vec3){requested[0],requested[1],requested[2]},c->contacts.planes,c->contacts.count);
 solved[0]=velocity.x;solved[1]=velocity.y;solved[2]=velocity.z;
 return c->contacts.count;
}
int bw_collision_contact_active(void* handle,int index){
 BwCollision* c=handle;return index>=0 && index<c->contacts.count && c->contacts.planes[index].push>0;
}
static int move_hull(BwCollision* c,float* position,float* velocity,float slope_cos,float dt){
 b3Vec3 p={position[0],position[1],position[2]},v={velocity[0],velocity[1],velocity[2]};
 b3Vec3 target=b3Add(p,b3MulSV(dt,v));
 for(int iteration=0;iteration<6;iteration++){
  hull_contacts(c,p);
  if(c->character_failed)return -1;
  // Recover overlap before casting; a sweep cannot recover an intersecting start.
  b3PlaneSolverResult recovery=solve_character_planes(b3Vec3_zero,&c->contacts);
  p=b3Add(p,recovery.delta);
  hull_contacts(c,p);
  if(c->character_failed)return -1;
  b3PlaneSolverResult solve=solve_character_planes(b3Sub(target,p),&c->contacts);
  b3Vec3 points[B3_MAX_HULL_VERTICES];const b3Vec3* local=b3GetHullPoints(c->mover);
  for(int i=0;i<c->mover->vertexCount;i++)points[i]=b3Add(p,local[i]);
  b3ShapeCastInput cast={0};cast.proxy=(b3ShapeProxy){points,c->mover->vertexCount,c->margin};
  cast.translation=solve.delta;cast.maxFraction=1;cast.canEncroach=true;
  for(int j=0;j<c->mesh_count;j++){
   if(!c->meshes[j].solid || query_ignores_index(c,j))continue;
   b3Mesh mesh={c->meshes[j].data,{1,1,1}};
   cast.proxy.radius=c->margin+c->meshes[j].radius;
   b3CastOutput hit=cast_static(&c->meshes[j],&cast);
   if(hit.hit)cast.maxFraction=hit.fraction;
  }
  b3Vec3 delta=b3MulSV(cast.maxFraction,solve.delta);p=b3Add(p,delta);
  if(b3LengthSquared(delta)<1e-10f)break;
 }
 hull_contacts(c,p);if(c->character_failed)return -1;
 solve_character_planes(b3MulSV(dt,v),&c->contacts);
 int grounded=0;
 for(int i=0;i<c->contacts.count;i++){
  b3Plane plane=c->contacts.planes[i].plane;
  // Uphill tangential motion can rise in world Y without leaving support.
  if(plane.normal.y>=slope_cos && plane.offset>=-0.001f && b3Dot(v,plane.normal)<=0.1f)grounded=1;
 }
 v=b3ClipVector(v,c->contacts.planes,c->contacts.count);
 position[0]=p.x;position[1]=p.y;position[2]=p.z;
 velocity[0]=v.x;velocity[1]=v.y;velocity[2]=v.z;return grounded;
}
int bw_collision_move(void* handle,float* position,float* velocity,float radius,float height,float slope_cos,float dt){
 if(((BwCollision*)handle)->mover)return move_hull(handle,position,velocity,slope_cos,dt);
 BwCollision* c=handle;b3Capsule capsule={{0,radius,0},{0,height-radius,0},radius};b3Pos p={position[0],position[1],position[2]};b3Vec3 v={velocity[0],velocity[1],velocity[2]};b3Pos target={p.x+v.x*dt,p.y+v.y*dt,p.z+v.z*dt};BwPlanes contacts=c->contacts;
 for(int i=0;i<6;i++){
  BwCapsulePlanes query={c,&contacts};
  contacts.count=0;b3World_CollideMover(c->world,p,&capsule,solid_query(),planes,&query);
  b3Vec3 wanted={target.x-p.x,target.y-p.y,target.z-p.z};b3PlaneSolverResult solve=b3SolvePlanes(wanted,contacts.planes,contacts.count);
  float fraction=b3World_CastMover(c->world,p,&capsule,solve.delta,solid_query(),mover_query_filter,c);
  b3Vec3 delta=b3MulSV(fraction,solve.delta);p.x+=delta.x;p.y+=delta.y;p.z+=delta.z;
  if(b3LengthSquared(delta)<1e-10f)break;
 }
 BwCapsulePlanes query={c,&contacts};
 contacts.count=0;b3World_CollideMover(c->world,p,&capsule,solid_query(),planes,&query);
 b3SolvePlanes(b3MulSV(dt,v),contacts.planes,contacts.count);
 int grounded=0;for(int i=0;i<contacts.count;i++){b3CollisionPlane* plane=&contacts.planes[i];if(plane->plane.normal.y>=slope_cos && plane->plane.offset>=-0.015f && b3Dot(v,plane->plane.normal)<=0.1f)grounded=1;}
 v=b3ClipVector(v,contacts.planes,contacts.count);
 c->contacts=contacts;
 position[0]=p.x;position[1]=p.y;position[2]=p.z;velocity[0]=v.x;velocity[1]=v.y;velocity[2]=v.z;return grounded;
}
