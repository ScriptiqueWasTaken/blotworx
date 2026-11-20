//! PC af58b0 package sequencing; scene updates and End side effects are owners.
#[derive(Debug,serde::Serialize)]
pub(super) struct State {pub phase:u32,pub index:usize,pub elapsed:f32}
impl Default for State {fn default()->Self {Self {phase:1,index:0,elapsed:0.}}}
#[derive(Debug,Clone,Copy,PartialEq,Eq)]
pub(super) enum Step {Scene(usize),Cleanup,Wait,End}
impl State {
    pub fn advance(&mut self,delta:f32,count:usize,flags:u32,released:bool,current_status:u32,cleanup_status:Option<u32>)->Step {
        self.elapsed+=delta;
        match self.phase {
            1=>{
                if self.index<count && current_status==2 && delta>0. {self.index+=1;}
                if self.index<count {return Step::Scene(self.index);}
                self.phase=if flags&16!=0 && !released {3}else{2};
                Step::Wait
            },
            2=>if cleanup_status.is_none_or(|status|status==2) {Step::End}else{Step::Cleanup},
            3=>{if released {self.phase=2;}Step::Wait},
            _=>Step::End,
        }
    }
    /// Rejected UpdateScene enters cleanup; rejected cleanup must call End.
    pub fn reject_update(&mut self)->Step {
        if self.phase==1 {self.phase=2;Step::Wait}else{Step::End}
    }
}
