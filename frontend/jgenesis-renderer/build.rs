//! Convert `mmpx_enhanced` shader from GLSL to WGSL

use naga::back::wgsl;
use naga::front::glsl;
use naga::valid::{Capabilities, ValidationFlags, Validator};
use naga::{FastHashMap, ShaderStage};
use std::error::Error;
use std::path::Path;
use std::{env, fs};

const MMPX_ENHANCED_GLSL: &str = include_str!("src/glsl/mmpx_enhanced.glsl");

fn main() -> Result<(), Box<dyn Error>> {
    let mmpx_enhanced_module = glsl::Frontend::default().parse(
        &glsl::Options { stage: ShaderStage::Compute, defines: FastHashMap::default() },
        MMPX_ENHANCED_GLSL,
    )?;

    let mmpx_enhanced_module_info = Validator::new(ValidationFlags::all(), Capabilities::all())
        .validate(&mmpx_enhanced_module)?;

    let mut mmpx_enhanced_wgsl = String::new();
    let mut writer = wgsl::Writer::new(&mut mmpx_enhanced_wgsl, wgsl::WriterFlags::all());

    writer.write(&mmpx_enhanced_module, &mmpx_enhanced_module_info)?;

    let out_dir = env::var("OUT_DIR")?;
    let out_path = Path::new(&out_dir).join("mmpx_enhanced.wgsl");
    fs::write(&out_path, &mmpx_enhanced_wgsl)?;

    println!("cargo::rerun-if-changed=src/glsl/mmpx_enhanced.glsl");

    Ok(())
}
