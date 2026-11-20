//! PC SHManager probe ownership/reuse/aging policy (b26a30..b27330).
//! Task submission/completion and light-repository updates remain explicit
//! upstream phases. A request here never fabricates completed coefficients.
use anyhow::{ensure,Context,Result};
use std::{collections::{BTreeMap,BTreeSet,VecDeque},ops::Range,sync::Arc};
use super::{d3d9_sh_query::{Request,frame_age},pc_float::{self,Precision}};
type Client=u64;
#[derive(Debug,Clone)]
struct Data {request:Request,raycast_owner:Option<Client>,index:usize}
/// Native cached-light cells have 24 initialized bits and one opaque byte.
/// Reset preserves that byte; task completion copies all four bytes.
#[derive(Debug,Clone,Copy)]
pub(super) struct CachedLight {id:u16,weight:u8,reserved:Option<u8>}
impl Default for CachedLight {fn default()->Self{Self {id:0xffff,weight:0,reserved:None}}}
impl CachedLight {
    /// Native cached entries are ordered by ascending quantized weight;
    /// equal weights retain source order, and insertion preserves the opaque
    /// byte of the destination cell while shifting other full cells.
    pub fn insert(cells:&mut [Self;9],id:u16,weight:u8) {
        let Some(index)=cells.iter().position(|cell|cell.id==0xffff||weight<cell.weight) else{return;};
        for next in (index+1..9).rev(){cells[next]=cells[next-1];}
        cells[index].id=id;cells[index].weight=weight;
    }
    pub fn reset(&mut self){self.id=0xffff;self.weight=0;}
    pub fn from_word(word:u32)->Self{Self {id:word as u16,weight:(word>>16) as u8,reserved:Some((word>>24) as u8)}}
    pub fn word(self)->Option<u32>{self.reserved.map(|r|u32::from(self.id)|(u32::from(self.weight)<<16)|(u32::from(r)<<24))}
}
#[derive(Debug)]
pub(super) struct TaskOutput {
    pub computed_frame:u32,pub coefficients:Arc<[[f32;3];9]>,pub cached_lights:[CachedLight;9],
}
#[derive(Debug,Clone)]
pub(super) struct TaskInput {pub index:usize,pub position:[f32;3],pub cached_lights:[CachedLight;9]}
#[derive(Debug,Default)]
struct Slot {data:Option<Data>,status:u32,requested_frame:u32,users:BTreeSet<Client>,
    result:Option<Arc<TaskOutput>>,cached_lights:[CachedLight;9],handle:super::d3d9_sh_query::Handle}

impl Slot {
    fn sync(&self)->Result<()> {
        self.handle.replace(super::d3d9_sh_query::Query {status:self.status,requested_frame:self.requested_frame,
            coefficients:self.result.as_ref().map(|result|result.coefficients.clone())})
    }
}
#[derive(Debug,Clone,Copy)]
pub(super) struct Outcome {pub slot:Option<usize>,pub needs_completion:bool}
#[derive(Debug)]
pub(super) struct Pool {
    slots:Vec<Slot>,active:VecDeque<usize>,inactive:VecDeque<usize>,free_data:BTreeSet<usize>,
    clients:BTreeMap<Client,usize>,block_size:usize,
    /// Native manager+25a0; set by nonempty task completion, not by a request.
    pub completed_frame:u32,
}
impl Pool {
    pub fn new(block_size:usize)->Result<Self> {
        ensure!(block_size>0,"Zero PC SH probe allocation block");
        let mut pool=Self {slots:vec![],active:VecDeque::new(),inactive:VecDeque::new(),free_data:BTreeSet::new(),clients:BTreeMap::new(),block_size,completed_frame:0};pool.grow()?;Ok(pool)
    }
    fn grow(&mut self)->Result<()> {
        let end=self.slots.len().checked_add(self.block_size).context("PC SH probe pool size overflow")?;
        self.slots.try_reserve(self.block_size).context("PC SH probe pool allocation failed")?;
        for id in self.slots.len()..end {self.slots.push(Slot::default());self.inactive.push_back(id);self.free_data.insert(id);}
        Ok(())
    }
    fn bind(&mut self,client:Client,id:usize) {
        let old=self.clients.insert(client,id);
        if let Some(old)=old {if old!=id {self.slots[old].users.remove(&client);}}
        self.slots[id].users.insert(client);
    }
    fn deactivate_many(&mut self,ids:Vec<usize>)->Result<()> {
        let retired=ids.iter().copied().collect::<BTreeSet<_>>();
        for id in ids {
            if let Some(data)=self.slots[id].data.take(){self.free_data.insert(data.index);}
            self.slots[id].status=0;self.slots[id].result=None;
            for cached in &mut self.slots[id].cached_lights {cached.reset();}
            self.inactive.push_back(id);self.slots[id].sync()?;
        }
        self.active.retain(|id|!retired.contains(id));
        // External clients still own the inactive handle. Native reset does
        // not decrement their references or clear last-requested frame.
        Ok(())
    }
    /// Native RemoveHandle clears the raycast-data owner only when it matches.
    /// Dropping an external reference does not immediately deactivate a probe.
    pub fn release(&mut self,client:Client) {
        if let Some(id)=self.clients.remove(&client) {
            if let Some(data)=self.slots[id].data.as_mut(){if data.raycast_owner==Some(client){data.raycast_owner=None;}}
            self.slots[id].users.remove(&client);
        }
    }
    /// Probe-aging portion of native Update. Repository update is separate.
    pub fn age(&mut self,frame:u32)->Result<()> {
        let retire=self.active.iter().copied().filter(|&id|frame_age(frame,self.slots[id].requested_frame)>=5||self.slots[id].users.is_empty()).collect::<Vec<_>>();
        self.deactivate_many(retire)
    }
    /// Native b26bf0 completion. `gate_open` is manager+28 !=0; the owning
    /// repository supplies it. It is not inferred from active probe count.
    /// The worker supplies its completed data-index range and raw outputs.
    pub fn complete_batch(&mut self,gate_open:bool,frame:u32,range:Range<usize>,outputs:&[Arc<TaskOutput>])->Result<()> {
        if !gate_open {return Ok(());}
        ensure!(range.start<=range.end&&range.end-range.start==outputs.len(),"Invalid PC SH completed batch range");
        let begin=range.start;
        self.complete_with(frame,range,|index|Some(outputs[index-begin].clone()))
    }
    /// Preserve physical task indices while omitting inactive kernel columns.
    /// Native worker columns are independent; no inactive output is consumed
    /// by completion. Missing active outputs are errors, never zero lighting.
    pub fn complete_sparse_batch(&mut self,gate_open:bool,frame:u32,range:Range<usize>,outputs:&BTreeMap<usize,Arc<TaskOutput>>)->Result<()> {
        if !gate_open{return Ok(());}
        ensure!(range.start<=range.end&&outputs.keys().all(|index|range.contains(index)),"Invalid sparse SH completion range");
        for &id in &self.active {
            let data=self.slots[id].data.as_ref().context("Active PC SH probe lost its task index")?;
            ensure!(!range.contains(&data.index)||outputs.contains_key(&data.index),"Missing active SH task output {}",data.index);
        }
        self.complete_with(frame,range,|index|outputs.get(&index).cloned())
    }
    fn complete_with(&mut self,frame:u32,range:Range<usize>,mut output:impl FnMut(usize)->Option<Arc<TaskOutput>>)->Result<()> {
        ensure!(self.active.iter().all(|&id|self.slots[id].data.is_some()),"Active PC SH probe lost its task index");
        for &id in &self.active {
            let index=self.slots[id].data.as_ref().unwrap().index;
            let result=if range.contains(&index){output(index)}else{None};
            let slot=&mut self.slots[id];
            if let Some(output)=result.as_ref().filter(|output|output.computed_frame==frame) {
                slot.cached_lights=output.cached_lights;slot.status=2;
            }else{slot.status=1;}
            // An in-range output pointer is retained even when its stamp is
            // stale; status1 prevents the native publisher from using it.
            slot.result=result;slot.sync()?;
        }
        self.completed_frame=frame;Ok(())
    }
    pub fn task_inputs(&self,range:Range<usize>)->Result<Vec<TaskInput>> {
        ensure!(range.start<=range.end&&range.end<=self.slots.len(),"Invalid SH task input range");
        let mut inputs=BTreeMap::new();
        for &id in &self.active {
            let slot=&self.slots[id];let data=slot.data.as_ref().context("Active PC SH probe lost its task input")?;
            if range.contains(&data.index) {
                ensure!(inputs.insert(data.index,TaskInput {index:data.index,position:data.request.position,cached_lights:slot.cached_lights}).is_none(),"Duplicate active SH physical task index");
            }
        }Ok(inputs.into_values().collect())
    }
    /// A view of the stable handle after the request/completion phase. Reading
    /// it does not refresh its last-requested timestamp or declare it ready.
    pub fn query_for_client(&self,client:Client)->Option<super::d3d9_sh_query::Handle> {
        Some(self.slots[*self.clients.get(&client)?].handle.clone())
    }
    /// Mode4 can reuse but cannot allocate. The completion flag is an ordered
    /// request to the real task phase, not a declaration that data is ready.
    pub fn request(&mut self,client:Client,request:Request,frame:u32,precision:Precision)->Result<Outcome> {
        ensure!(request.position.iter().all(|v|v.is_finite()),"Nonfinite PC SH probe request");
        ensure!((1..=4).contains(&request.mode),"Unverified PC SH probe request mode {}",request.mode);
        let old=self.clients.get(&client).copied();let mut chosen=None;
        if let Some(id)=old {
            if self.slots[id].status!=0&&self.slots[id].users.len()<=1 {
                let data=self.slots[id].data.as_mut().context("Active PC SH probe has no data")?;
                data.request.position=request.position;
                if request.mode!=4 {data.request.mode=request.mode;}
                data.raycast_owner=Some(client);chosen=Some(id);
            }
        }
        if chosen.is_none() {
            for &id in &self.active {
                let data=self.slots[id].data.as_mut().context("Listed PC SH probe has no data")?;
                if pc_float::sh_probe_displacement(data.request.position,request.position,precision)?<0.010000000707805157f32 {
                    if request.mode!=4&&(request.mode as i32)>(data.request.mode as i32){data.request.mode=request.mode;}
                    chosen=Some(id);break;
                }
            }
        }
        if chosen.is_none()&&request.mode!=4 {
            if self.inactive.is_empty() {
                let unused=self.active.iter().copied().filter(|&id|self.slots[id].users.is_empty()).collect::<Vec<_>>();
                self.deactivate_many(unused)?;
            }
            if self.inactive.is_empty(){self.grow()?;}
            let index=self.free_data.pop_first().context("PC SH probe pool has no free data slot")?;
            let id=self.inactive.pop_front().context("PC SH probe pool has no free handle")?;
            self.slots[id].data=Some(Data {request,raycast_owner:Some(client),index});self.slots[id].status=1;
            self.active.push_back(id);chosen=Some(id);
        }
        if let Some(id)=chosen {self.slots[id].requested_frame=frame;self.bind(client,id);self.slots[id].sync()?;}
        Ok(Outcome {slot:chosen,needs_completion:chosen.is_some()&&request.mode==4&&frame!=self.completed_frame})
    }
}
/// Per-client native virtual-slot9 service over the shared pool. The task
/// phase is required explicitly whenever the original manager would wait.
pub(super) struct Service<'a,C> {pub pool:&'a mut Pool,pub client:u64,pub frame:u32,pub precision:Precision,pub complete:C}
impl<C:FnMut(&mut Pool,u32)->Result<()>> super::d3d9_sh_query::Manager for Service<'_,C> {
    fn request(&mut self,request:Request,query:&mut Option<super::d3d9_sh_query::Handle>,_:&mut u32)->Result<bool> {
        let outcome=self.pool.request(self.client,request,self.frame,self.precision)?;
        if outcome.slot.is_none(){return Ok(false);}
        *query=self.pool.query_for_client(self.client);
        if outcome.needs_completion {(self.complete)(self.pool,self.frame)?;}
        Ok(true)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    impl Pool {
        fn snapshot(&self,complete:&[u32],seen:&BTreeSet<Client>)->serde_json::Value {
            let slots=self.slots.iter().map(|slot|serde_json::json!({"status":slot.status,"refs":1+slot.users.len(),"requested_frame":slot.requested_frame,
                "data_index":slot.data.as_ref().map(|d|d.index),"mode":slot.data.as_ref().map(|d|d.request.mode),"position_words":slot.data.as_ref().map(|d|d.request.position.map(f32::to_bits)),"raycast_owner":slot.data.as_ref().and_then(|d|d.raycast_owner)})).collect::<Vec<_>>();
            serde_json::json!({"active":self.active,"inactive":self.inactive,"slots":slots,"clients":seen.iter().map(|key|(key.to_string(),self.clients.get(key).copied())).collect::<BTreeMap<_,_>>(),"complete_calls":complete,"capacity":self.slots.len()})
        }
    }
    #[test]
    fn shared_clients_observe_request_stamps_and_completed_coefficients()->Result<()> {
        use super::super::d3d9_sh_query::{Client,Publication};
        let mut pool=Pool::new(2)?;
        let mut a=Client {position:[1.,0.,0.],query:None,token:0};let mut b=Client {position:[1.05,0.,0.],query:None,token:0};
        let unexpected=|_:&mut Pool,_|->Result<()>{anyhow::bail!("Unexpected task wait")};
        for (id,client) in [(1,&mut a),(2,&mut b)] {
            assert!(!client.request(&mut Service {pool:&mut pool,client:id,frame:1,precision:Precision::Extended,complete:unexpected},false)?);
        }
        assert!(a.query.as_ref().unwrap().same(b.query.as_ref().unwrap()));
        let output=|frame,value|Arc::new(TaskOutput {computed_frame:frame,coefficients:Arc::new([[value;3];9]),cached_lights:[0x170001;9].map(CachedLight::from_word)});
        pool.complete_batch(true,1,0..1,&[output(1,0.2)])?;
        assert_eq!(b.query.as_ref().unwrap().read()?.status,2);
        assert!(a.request(&mut Service {pool:&mut pool,client:1,frame:2,precision:Precision::Extended,complete:unexpected},false)?);
        assert_eq!(b.query.as_ref().unwrap().read()?.requested_frame,2);
        let mut snapshot=super::super::d3d9_sh::Snapshot {coefficients:[[0.;3];9],specular_color:None,specular_direction:None};
        let mut service=Service {pool:&mut pool,client:2,frame:2,precision:Precision::Extended,complete:|pool:&mut Pool,frame|pool.complete_batch(true,frame,0..1,&[output(frame,0.3)])};
        assert!(b.publish(Publication {resource_kind:0x8000,capability:None},2,Some(&mut service),Ok,&mut snapshot)?);
        assert_eq!(snapshot.coefficients,[[0.3;3];9]);assert_eq!(a.query.as_ref().unwrap().read()?.coefficients.unwrap()[0],[0.3;3]);Ok(())
    }
    #[test]
    fn original_native_completion_range_and_frame_rules()->Result<()> {
        let fixture:serde_json::Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/pc_sh_completion.json"))?;
        let coefficients=|value:&serde_json::Value|Arc::new(std::array::from_fn(|i|std::array::from_fn(|j|f32::from_bits(value[i*3+j].as_u64().unwrap() as u32))));
        let cached=|value:&serde_json::Value|std::array::from_fn(|i|value[i].as_u64().unwrap() as u32);
        for row in fixture["cases"].as_array().unwrap() {
            let mut pool=Pool::new(5)?;pool.completed_frame=17;
            for (id,initial) in row["initial"].as_array().unwrap().iter().enumerate() {
                pool.request(id as u64,Request {position:[id as f32*10.,0.,0.],mode:2},1,Precision::Extended)?;
                let slot=&mut pool.slots[id];slot.status=2;slot.requested_frame=initial["requested_frame"].as_u64().unwrap() as u32;
                slot.data.as_mut().unwrap().index=initial["index"].as_u64().unwrap() as usize;
                slot.cached_lights=cached(&initial["cached"]).map(CachedLight::from_word);
                slot.result=Some(Arc::new(TaskOutput {computed_frame:88,coefficients:coefficients(&initial["coefficients"]),cached_lights:cached(&initial["cached"]).map(CachedLight::from_word)}));slot.sync()?;
            }
            let outputs=row["outputs"].as_array().unwrap().iter().map(|output|Arc::new(TaskOutput {computed_frame:output["frame"].as_u64().unwrap() as u32,coefficients:coefficients(&output["coefficients"]),cached_lights:cached(&output["cached"]).map(CachedLight::from_word)})).collect::<Vec<_>>();
            pool.complete_batch(row["gate"].as_bool().unwrap(),row["frame"].as_u64().unwrap() as u32,row["begin"].as_u64().unwrap() as usize..row["end"].as_u64().unwrap() as usize,&outputs)?;
            assert_eq!(pool.completed_frame,row["completed_frame"].as_u64().unwrap() as u32,"{row}");
            let actual=pool.active.iter().map(|&id|{let slot=&pool.slots[id];serde_json::json!({"status":slot.status,"requested_frame":slot.requested_frame,
                "coefficients":slot.result.as_ref().map(|r|r.coefficients.iter().flatten().map(|v|v.to_bits()).collect::<Vec<_>>()),
                "cached":slot.cached_lights.map(|c|c.word().unwrap())})}).collect::<Vec<_>>();
            assert_eq!(serde_json::json!(actual),row["after"],"{row}");
            if !pool.active.is_empty() {
                let old_reserved=pool.slots[0].cached_lights[0].reserved;pool.age(1000)?;
                assert_eq!(pool.slots[0].cached_lights[0].id,0xffff);assert_eq!(pool.slots[0].cached_lights[0].weight,0);assert_eq!(pool.slots[0].cached_lights[0].reserved,old_reserved);assert!(pool.slots[0].result.is_none());
                assert_eq!(pool.query_for_client(0).unwrap().read()?.status,0);
            }
        }Ok(())
    }
    #[test]
    fn released_clients_do_not_accumulate_tombstones()->Result<()> {
        let mut pool=Pool::new(2)?;
        for client in 0..1000 {
            pool.request(client,Request {position:[client as f32,0.,0.],mode:2},1,Precision::Extended)?;
            pool.release(client);
        }
        assert!(pool.clients.is_empty());assert_eq!(pool.slots.len(),2);
        pool.age(1)?;assert!(pool.active.is_empty());assert_eq!(pool.inactive.len(),2);Ok(())
    }
    #[test]
    fn real_pc_pool_lifecycle_transcript()->Result<()> {
        let fixture:serde_json::Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/pc_sh_pool_lifecycle.json"))?;
        let mut pool=Pool::new(fixture["block_size"].as_u64().unwrap() as usize)?;let mut frame=fixture["initial_frame"].as_u64().unwrap() as u32;let mut complete=vec![];let mut seen=BTreeSet::new();
        for step in fixture["steps"].as_array().unwrap() {
            let command=&step["command"];if let Some(client)=command["client"].as_u64(){seen.insert(client);}
            match command["op"].as_str().unwrap() {
                "request"=>{
                    let result=pool.request(command["client"].as_u64().unwrap(),Request {position:std::array::from_fn(|i|command["position"][i].as_f64().unwrap() as f32),mode:command["mode"].as_u64().unwrap() as u32},frame,Precision::Extended)?;
                    assert_eq!(result.slot.is_some(),step["accepted"].as_bool().unwrap(),"{command}");if result.needs_completion {complete.push(frame);}
                },
                "release"=>pool.release(command["client"].as_u64().unwrap()),
                "update"=>{frame=command["frame"].as_u64().unwrap() as u32;pool.age(frame)?;},
                _=>unreachable!(),
            }
            assert_eq!(pool.snapshot(&complete,&seen),step["after"],"{command}");
        }Ok(())
    }
}
