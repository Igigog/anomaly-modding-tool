extern crate winres;

fn main() {
    if cfg!(target_os = "windows") && !cfg!(debug_assertions) {
        let mut res = winres::WindowsResource::new();
        res.set_icon("resources\\yawn.ico");
        res.compile().unwrap();
    }
}
