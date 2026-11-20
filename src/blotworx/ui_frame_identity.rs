// Exact comparison of the retained HUD render target's inputs. No hashes,
// rounding or time-based throttling. Unknown/mutable GPU operations decline.
use ruffle_render::{commands::{Command,CommandList,RenderBlendMode},matrix::Matrix,transform::Transform};
fn matrix(a:&Matrix,b:&Matrix)->bool {
    [a.a,a.b,a.c,a.d].map(f32::to_bits)==[b.a,b.b,b.c,b.d].map(f32::to_bits)
        && a.tx==b.tx && a.ty==b.ty
}
fn transform(a:&Transform,b:&Transform)->bool {
    a.perspective_projection.is_none() && b.perspective_projection.is_none()
        && matrix(&a.matrix,&b.matrix) && a.tz.to_bits()==b.tz.to_bits()
        && a.color_transform==b.color_transform
}
pub fn commands(a:&CommandList,b:&CommandList)->bool {
    a.commands.len()==b.commands.len() && a.commands.iter().zip(&b.commands).all(|(a,b)| {
        match(a,b) {
            (Command::RenderBitmap{bitmap:a,transform:at,smoothing:as_,pixel_snapping:ap,region:ar},
             Command::RenderBitmap{bitmap:b,transform:bt,smoothing:bs,pixel_snapping:bp,region:br})=>
                a==b && transform(at,bt) && as_==bs && ap==bp && ar==br,
            (Command::RenderShape{shape:a,transform:at},Command::RenderShape{shape:b,transform:bt})=>
                std::sync::Arc::ptr_eq(&a.0,&b.0) && transform(at,bt),
            (Command::DrawRect{color:a,matrix:am},Command::DrawRect{color:b,matrix:bm}) |
            (Command::DrawLine{color:a,matrix:am},Command::DrawLine{color:b,matrix:bm}) |
            (Command::DrawLineRect{color:a,matrix:am},Command::DrawLineRect{color:b,matrix:bm})=>a==b && matrix(am,bm),
            (Command::RenderAlphaMask{maskee_commands:a,mask_commands:am},Command::RenderAlphaMask{maskee_commands:b,mask_commands:bm})=>
                commands(a,b) && commands(am,bm),
            (Command::Blend(a,RenderBlendMode::Builtin(ab)),Command::Blend(b,RenderBlendMode::Builtin(bb)))=>
                ab==bb && commands(a,b),
            (Command::PushMask,Command::PushMask) | (Command::ActivateMask,Command::ActivateMask) |
            (Command::DeactivateMask,Command::DeactivateMask) | (Command::PopMask,Command::PopMask)=>true,
            _=>false,
        }
    })
}
#[cfg(test)]mod tests {
    use super::*;
    use ruffle_render::{commands::CommandHandler,bitmap::{BitmapHandle,BitmapHandleImpl,PixelSnapping,PixelRegion}};
    use std::sync::Arc;
    #[derive(Debug)]struct Image;
    impl BitmapHandleImpl for Image {}
    #[test]fn exact_bits_masks_and_order_are_required() {
        let mut a=CommandList::new();a.draw_rect(swf::Color::WHITE,Matrix::IDENTITY);
        assert!(commands(&a,&a));
        let mut b=a.clone();
        if let Command::DrawRect{matrix,..}=&mut b.commands[0]{matrix.b=-0.;}
        assert!(!commands(&a,&b));
        b=a.clone();b.push_mask();assert!(!commands(&a,&b));
        let mut mask=CommandList::new();mask.render_alpha_mask(a.clone(),a.clone());
        assert!(commands(&mask,&mask));
        let mut b=mask.clone();if let Command::RenderAlphaMask{mask_commands,..}=&mut b.commands[0]{mask_commands.commands.push(Command::PopMask);}
        assert!(!commands(&mask,&b));
    }
    #[test]fn bitmaps_require_same_resource_region_and_complete_transform() {
        let bitmap=BitmapHandle(Arc::new(Image));
        let mut a=CommandList::new();a.render_bitmap(bitmap,Transform::default(),true,PixelSnapping::Auto,PixelRegion::for_whole_size(4,4));
        assert!(commands(&a,&a));
        let mut b=a.clone();if let Command::RenderBitmap{bitmap,..}=&mut b.commands[0]{*bitmap=BitmapHandle(Arc::new(Image));}
        assert!(!commands(&a,&b));
        let mut b=a.clone();if let Command::RenderBitmap{region,..}=&mut b.commands[0]{region.x_max-=1;}
        assert!(!commands(&a,&b));
        let mut b=a.clone();if let Command::RenderBitmap{transform,..}=&mut b.commands[0]{transform.tz=-0.;}
        assert!(!commands(&a,&b));
        let mut b=a.clone();if let Command::RenderBitmap{smoothing,..}=&mut b.commands[0]{*smoothing=false;}
        assert!(!commands(&a,&b));
    }
    #[test]fn mutable_stage3d_is_never_reused() {
        let mut a=CommandList::new();a.render_stage3d(BitmapHandle(Arc::new(Image)),Transform::default());
        assert!(!commands(&a,&a));
        let mut b=CommandList::new();b.blend(a,RenderBlendMode::Builtin(swf::BlendMode::Normal));
        assert!(!commands(&b,&b));
    }
}
