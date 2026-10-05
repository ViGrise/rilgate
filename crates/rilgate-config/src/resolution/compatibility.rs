pub(crate) fn normalize_internal_prefix(mut prefix: String) -> String {
  if !prefix.starts_with('/') {
    prefix.insert(0, '/');
  }

  while prefix.len() > 1 && prefix.ends_with('/') {
    prefix.pop();
  }

  if prefix == "/" {
    panic!("Gateway namespace must reserve a non-root path");
  }

  prefix
}
