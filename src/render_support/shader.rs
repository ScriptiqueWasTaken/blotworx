pub fn new_shader() -> String {
    let mut s=String::from("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<MaterialTemplateData xmlns=\"Nintendo.NW4R.3DEditor.MaterialTemplate\" version=\"1.7.0\" Applyable=\"true\">\n<Material Applyable=\"true\">\n<DisplayFace DisplayFace=\"Front\" Applyable=\"true\"/>\n<Channel AlphaFunc=\"Clamp\" DiffuseFunc=\"Clamp\" Applyable=\"true\" LightSetIndex=\"0\" SpecularLighting=\"false\" AlphaSrc=\"Vertex\" DiffuseSrc=\"Vertex\" AlphaLighting=\"false\" DiffuseAmbientLighting=\"false\">\n");
    for name in ["Mat0Color", "Amb0Color", "Mat1Color", "Amb1Color"] {
        s += &format!("<{name} R=\"255\" G=\"255\" B=\"255\" A=\"255\" Applyable=\"true\"/>\n");
    }
    s+="</Channel>\n<TextureMaps Applyable=\"true\"/>\n<Tev Applyable=\"true\"><SwapTables Applyable=\"true\">\n";
    for rgb in [
        ["R", "G", "B"],
        ["R", "R", "R"],
        ["G", "G", "G"],
        ["B", "B", "B"],
    ] {
        s += &format!(
            "<SwapTable R=\"{}\" G=\"{}\" B=\"{}\" A=\"A\" Applyable=\"true\"/>\n",
            rgb[0], rgb[1], rgb[2]
        );
    }
    s += "</SwapTables>\n<ColorRegisters Applyable=\"true\">\n";
    for _ in 0..3 {
        s += "<ColorRegister R=\"0\" G=\"0\" B=\"0\" A=\"0\" Applyable=\"true\"/>\n";
    }
    s += "</ColorRegisters>\n<ConstantRegisters Applyable=\"true\">\n";
    for _ in 0..4 {
        s += "<ConstantRegister R=\"0\" G=\"0\" B=\"0\" A=\"0\" Applyable=\"true\"/>\n";
    }
    s+="</ConstantRegisters>\n<Stages Applyable=\"true\"><Stage Applyable=\"true\">\n<Order RasOrder=\"Color0A0\" TexMapOrder=\"-1\" TexCoordOrder=\"-1\"/>\n<Swap RasSwap=\"0\" TexMapSwap=\"0\"/>\n";
    for (stage, input, ksel) in [("ColorStage", "RasC", "K0"), ("AlphaStage", "RasA", "K0A")] {
        s+=&format!("<{stage} A=\"{input}\" B=\"Zero\" C=\"Zero\" D=\"Zero\" KSel=\"{ksel}\" Op=\"Add\" Bias=\"Zero\" Scale=\"Scale1\" Clamp=\"true\" OutReg=\"Prev\"/>\n");
    }
    s+="<IndirectStage IndStage=\"0\" Format=\"Itf8\" BiasSel=\"ItbNone\" MatrixSel=\"ItmOff\" WrapS=\"ItwOff\" WrapT=\"ItwOff\" AddPrev=\"false\" UtcLod=\"false\" AlphaSel=\"ItbaOff\"/>\n</Stage></Stages></Tev>\n<Fog FogIndex=\"0\" Enable=\"true\" Applyable=\"true\"/>\n<PE Applyable=\"true\" DrawMode=\"Opa\">\n<ACompare Comp0=\"Always\" Ref0=\"0\" Op=\"And\" Comp1=\"Always\" Ref1=\"0\"/>\n<DstAlpha Enable=\"false\" Alpha=\"0\"/>\n<ZCompare Enable=\"true\" Func=\"Lequal\" Update=\"true\" BeforeTex=\"true\"/>\n<Blend Type=\"None\" SrcFactor=\"SrcAlpha\" DstFactor=\"InvSrcAlpha\" LogicOp=\"Copy\"/>\n</PE>\n<UserDataList Applyable=\"true\"/>\n<ConvertOption TevShareEnable=\"true\" CompressEnable=\"true\" Applyable=\"true\"/>\n<WorkData Applyable=\"true\" Comment=\"\"><Color R=\"255\" G=\"255\" B=\"255\" A=\"255\" Applyable=\"true\"/></WorkData>\n</Material>\n<Textures Applyable=\"true\"/>\n<Palettes Applyable=\"true\"/>\n</MaterialTemplateData>\n";
    s
}
