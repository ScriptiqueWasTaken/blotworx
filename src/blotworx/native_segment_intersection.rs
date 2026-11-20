//! Geometry-only459cbc; callers retain native accounting and output contracts.
pub fn intersection(a:[f32;3],b:[f32;3],c:[f32;3],d:[f32;3],first:Option<&mut[f32;3]>,second:Option<&mut[f32;3]>)->bool{
    for axis in [0,2] {
        if (a[axis]<c[axis] && b[axis]<c[axis] && a[axis]<d[axis] && b[axis]<d[axis]) ||
            (a[axis]>c[axis] && b[axis]>c[axis] && a[axis]>d[axis] && b[axis]>d[axis]) {return false}
    }
    let ez=d[2]-c[2];let ex=d[0]-c[0];let rz=b[2]-a[2];let rx=b[0]-a[0];
    let left=rz*ex;let right=rx*ez;
    if left-right<0. {return false}
    let ox=a[0]-c[0];let oz=a[2]-c[2];let denominator=right-left;
    if denominator==0. {return false}
    let t=(ex*oz-ez*ox)/denominator;
    if t<0. || !(t<=1.) {return false}
    let u=(rx*oz-rz*ox)/denominator;
    if u<0. || !(u<=1.) {return false}
    let x=a[0]+t*rx;let z=a[2]+t*rz;
    if let Some(first)=first{*first=[x,c[1]+u*(d[1]-c[1]),z];}
    if let Some(second)=second{*second=[x,a[1]+t*(b[1]-a[1]),z];}
    true
}