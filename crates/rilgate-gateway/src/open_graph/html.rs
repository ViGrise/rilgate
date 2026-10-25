/// The attributes of one `<meta>` element, as source text before entity decoding.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct RawMeta {
  pub property: Option<String>,
  pub name: Option<String>,
  pub content: Option<String>,
}

/// Scans a buffer that grows by appended chunks. Each call resumes at the
/// first markup construct that was incomplete, so a construct split across
/// chunks is read once, when it is whole.
#[derive(Default)]
pub(super) struct HeadScanner {
  position: usize,
  done: bool,
}

enum Step {
  Next(usize),
  Pending,
  Done,
}

// Elements whose content is text up to their end tag, so markup inside them is not parsed.
const RAW_TEXT: [&str; 6] = [
  "script", "style", "title", "textarea", "noscript", "template",
];

impl HeadScanner {
  /// Reports each complete `<meta>` element and returns `true` once the head has ended.
  pub fn scan(&mut self, buffer: &[u8], found: &mut impl FnMut(RawMeta)) -> bool {
    while !self.done {
      let Some(start) = find_byte(buffer, self.position, b'<') else {
        self.position = buffer.len();
        break;
      };
      match step(buffer, start, found) {
        Step::Next(next) => self.position = next,
        Step::Pending => {
          self.position = start;
          break;
        }
        Step::Done => self.done = true,
      }
    }
    self.done
  }
}

fn step(buffer: &[u8], start: usize, found: &mut impl FnMut(RawMeta)) -> Step {
  let rest = &buffer[start..];
  let Some(&next) = rest.get(1) else {
    return Step::Pending;
  };
  match next {
    b'!' if rest.starts_with(b"<!--") => {
      // Searching from the opening dashes also ends the empty comments `<!-->` and `<!--->`.
      find(buffer, start + 2, b"-->").map_or(Step::Pending, |end| Step::Next(end + 3))
    }
    b'!' if b"<!--".starts_with(rest) => Step::Pending,
    b'!' | b'?' => bogus_comment(buffer, start + 2),
    b'/' => match rest.get(2) {
      None => Step::Pending,
      Some(byte) if byte.is_ascii_alphabetic() => {
        let Some((name, end)) = tag_name(buffer, start + 2) else {
          return Step::Pending;
        };
        if name == "head" {
          return Step::Done;
        }
        attributes(buffer, end, None).map_or(Step::Pending, Step::Next)
      }
      Some(_) => bogus_comment(buffer, start + 2),
    },
    byte if byte.is_ascii_alphabetic() => {
      let Some((name, end)) = tag_name(buffer, start + 1) else {
        return Step::Pending;
      };
      if name == "body" {
        return Step::Done;
      }
      let mut meta = RawMeta::default();
      let Some(end) = attributes(buffer, end, (name == "meta").then_some(&mut meta)) else {
        return Step::Pending;
      };
      if name == "meta" {
        found(meta);
      } else if RAW_TEXT.contains(&name.as_str()) {
        return raw_text_end(buffer, end, &name).map_or(Step::Pending, Step::Next);
      }
      Step::Next(end)
    }
    _ => Step::Next(start + 1),
  }
}

fn bogus_comment(buffer: &[u8], from: usize) -> Step {
  find_byte(buffer, from, b'>').map_or(Step::Pending, |end| Step::Next(end + 1))
}

/// Reads a lowercase tag name and returns it with the index after it.
fn tag_name(buffer: &[u8], from: usize) -> Option<(String, usize)> {
  let length = buffer
    .get(from..)?
    .iter()
    .position(|&byte| is_space(byte) || byte == b'/' || byte == b'>')?;
  let name = String::from_utf8_lossy(&buffer[from..from + length]).to_ascii_lowercase();
  Some((name, from + length))
}

/// Reads attributes up to the end of a tag and returns the index after its `>`.
/// The first occurrence of an attribute wins, as in HTML parsing.
fn attributes(buffer: &[u8], mut at: usize, mut meta: Option<&mut RawMeta>) -> Option<usize> {
  loop {
    while buffer
      .get(at)
      .is_some_and(|&byte| is_space(byte) || byte == b'/')
    {
      at += 1;
    }
    if *buffer.get(at)? == b'>' {
      return Some(at + 1);
    }
    let name_start = at;
    // The first byte of a name can be `=`.
    at += 1;
    while buffer
      .get(at)
      .is_some_and(|&byte| !is_space(byte) && !matches!(byte, b'=' | b'>' | b'/'))
    {
      at += 1;
    }
    let name = &buffer[name_start..at];
    while buffer.get(at).is_some_and(|&byte| is_space(byte)) {
      at += 1;
    }
    let mut value: &[u8] = &[];
    if *buffer.get(at)? == b'=' {
      at += 1;
      while buffer.get(at).is_some_and(|&byte| is_space(byte)) {
        at += 1;
      }
      match *buffer.get(at)? {
        quote @ (b'"' | b'\'') => {
          let end = find_byte(buffer, at + 1, quote)?;
          value = &buffer[at + 1..end];
          at = end + 1;
        }
        b'>' => {}
        _ => {
          let start = at;
          while buffer
            .get(at)
            .is_some_and(|&byte| !is_space(byte) && byte != b'>')
          {
            at += 1;
          }
          buffer.get(at)?;
          value = &buffer[start..at];
        }
      }
    }
    if let Some(meta) = meta.as_deref_mut() {
      let field = if name.eq_ignore_ascii_case(b"property") {
        Some(&mut meta.property)
      } else if name.eq_ignore_ascii_case(b"name") {
        Some(&mut meta.name)
      } else if name.eq_ignore_ascii_case(b"content") {
        Some(&mut meta.content)
      } else {
        None
      };
      if let Some(field) = field.filter(|field| field.is_none()) {
        *field = Some(String::from_utf8_lossy(value).into_owned());
      }
    }
  }
}

/// Returns the index of the end tag that closes a raw text element.
fn raw_text_end(buffer: &[u8], mut at: usize, name: &str) -> Option<usize> {
  loop {
    let start = find(buffer, at, b"</")?;
    let after = start + 2 + name.len();
    if buffer
      .get(start + 2..after)?
      .eq_ignore_ascii_case(name.as_bytes())
    {
      let byte = *buffer.get(after)?;
      if is_space(byte) || byte == b'/' || byte == b'>' {
        return Some(start);
      }
    }
    at = start + 2;
  }
}

fn find_byte(buffer: &[u8], from: usize, byte: u8) -> Option<usize> {
  let offset = buffer
    .get(from..)?
    .iter()
    .position(|&value| value == byte)?;
  Some(from + offset)
}

fn find(buffer: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
  let offset = buffer
    .get(from..)?
    .windows(needle.len())
    .position(|window| window == needle)?;
  Some(from + offset)
}

fn is_space(byte: u8) -> bool {
  matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | 0x0c)
}

#[cfg(test)]
mod tests {
  use super::*;

  fn scan_chunks(chunks: &[&str]) -> (Vec<RawMeta>, bool) {
    let mut scanner = HeadScanner::default();
    let mut buffer = Vec::new();
    let mut found = Vec::new();
    let mut done = false;
    for chunk in chunks {
      buffer.extend_from_slice(chunk.as_bytes());
      done = scanner.scan(&buffer, &mut |meta| found.push(meta));
    }
    (found, done)
  }

  fn contents(found: &[RawMeta]) -> Vec<&str> {
    found
      .iter()
      .filter_map(|meta| meta.content.as_deref())
      .collect()
  }

  #[test]
  fn markup_inside_text_elements_and_comments_is_not_metadata() {
    let (found, done) = scan_chunks(&[concat!(
      "<head><!-- <meta property=og:title content=comment> -->",
      "<script>let s = '<meta property=\"og:title\" content=\"script\">';</script>",
      "<title><meta property=og:title content=title></title>",
      "<template><meta property=og:title content=template></template>",
      "<meta property=og:title content=real></head><meta content=after>",
    )]);
    assert_eq!(contents(&found), ["real"]);
    assert!(done);
  }

  #[test]
  fn quoted_values_and_repeated_attributes_follow_html_parsing() {
    let (found, _) =
      scan_chunks(&["<META Property='og:title' content=\"a > b\" content=second NAME=x>"]);
    assert_eq!(
      found,
      [RawMeta {
        property: Some("og:title".into()),
        name: Some("x".into()),
        content: Some("a > b".into()),
      }]
    );
  }

  #[test]
  fn constructs_split_across_chunks_are_read_once_when_whole() {
    let (found, done) = scan_chunks(&[
      "<head><meta property=og:title con",
      "tent='one'><!-",
      "- <meta content=hidden> -",
      "-><meta content=two></he",
      "ad>",
    ]);
    assert_eq!(contents(&found), ["one", "two"]);
    assert!(done);
  }

  #[test]
  fn an_unterminated_tag_at_the_end_of_input_is_dropped() {
    let (found, done) = scan_chunks(&["<head><meta content=one><meta content='two"]);
    assert_eq!(contents(&found), ["one"]);
    assert!(!done);
  }

  #[test]
  fn the_head_ends_at_the_body_start_tag() {
    let (found, done) =
      scan_chunks(&["<meta content=one><header></header><body><meta content=two>"]);
    assert_eq!(contents(&found), ["one"]);
    assert!(done);
  }
}
