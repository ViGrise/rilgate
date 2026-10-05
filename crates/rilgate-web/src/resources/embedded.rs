use include_dir::{Dir, include_dir};
use std::collections::BTreeMap;
static WEB: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/../../web");

pub struct EmbeddedWeb;
impl EmbeddedWeb {
  pub fn get(name: &str) -> Option<&'static [u8]> {
    WEB.get_file(name).map(|file| file.contents())
  }
  pub fn entries() -> BTreeMap<String, &'static [u8]> {
    fn collect(dir: &'static Dir<'static>, out: &mut BTreeMap<String, &'static [u8]>) {
      for file in dir.files() {
        out.insert(
          file.path().to_string_lossy().replace('\\', "/"),
          file.contents(),
        );
      }
      for child in dir.dirs() {
        collect(child, out);
      }
    }
    let mut out = BTreeMap::new();
    collect(&WEB, &mut out);
    out
  }
}
