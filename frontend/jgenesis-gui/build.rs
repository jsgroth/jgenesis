use std::env;
use std::error::Error;
use winresource::WindowsResource;

fn main() -> Result<(), Box<dyn Error>> {
    if env::var("CARGO_CFG_TARGET_OS").is_ok_and(|var| var.as_str() == "windows")
        && env::var("CARGO_FEATURE_WINDOWS_ICON").is_ok()
    {
        write_windows_resource_file()?;
    }

    Ok(())
}

fn write_windows_resource_file() -> Result<(), Box<dyn Error>> {
    let mut resource = WindowsResource::new();
    resource.set_icon("../jgenesis.ico");
    resource.compile()?;

    Ok(())
}
