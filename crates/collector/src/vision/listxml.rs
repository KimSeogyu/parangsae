//! S3 ListObjects v1 XML. Parsed once into keys and sizes.

use quick_xml::events::Event;
use quick_xml::Reader;

use crate::Error;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListedObject {
    pub key: String,
    pub size: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListPage {
    pub objects: Vec<ListedObject>,
    pub truncated: bool,
}

pub fn list_page(xml: &str) -> Result<ListPage, Error> {
    let mut reader = Reader::from_str(xml);
    let mut buf = Vec::new();
    let mut objects = Vec::new();
    let mut truncated = false;
    let mut key = String::new();
    let mut size = None;
    let mut text_target: Option<TextTarget> = None;
    loop {
        let event = reader
            .read_event_into(&mut buf)
            .map_err(|err| Error::Xml(err.to_string()))?;
        match event {
            Event::Start(element) => {
                let name = local_name(element.name().as_ref());
                text_target = TextTarget::from_name(&name);
                if name == "Contents" {
                    key.clear();
                    size = None;
                }
            }
            Event::Empty(_) => {}
            Event::Text(text) => {
                let value = quick_xml::escape::unescape(text.xml10_content().as_ref())
                    .map_err(|err| Error::Xml(err.to_string()))?
                    .into_owned();
                match text_target {
                    Some(TextTarget::Key) => key = value,
                    Some(TextTarget::Size) => {
                        size =
                            Some(value.parse::<u64>().map_err(|_| {
                                Error::Xml(format!("size is not an integer: {value}"))
                            })?);
                    }
                    Some(TextTarget::Truncated) => {
                        truncated = value == "true";
                    }
                    None => {}
                }
                text_target = None;
            }
            Event::End(element) => {
                if local_name(element.name().as_ref()) == "Contents" && !key.is_empty() {
                    let object_size =
                        size.ok_or_else(|| Error::Xml(format!("object {key} has no size")))?;
                    objects.push(ListedObject {
                        key: key.clone(),
                        size: object_size,
                    });
                    key.clear();
                    size = None;
                }
            }
            Event::Eof => break,
            Event::CData(_)
            | Event::Comment(_)
            | Event::Decl(_)
            | Event::PI(_)
            | Event::DocType(_)
            | Event::GeneralRef(_) => {}
        }
        buf.clear();
    }
    Ok(ListPage { objects, truncated })
}

enum TextTarget {
    Key,
    Size,
    Truncated,
}

impl TextTarget {
    fn from_name(name: &str) -> Option<Self> {
        match name {
            "Key" => Some(Self::Key),
            "Size" => Some(Self::Size),
            "IsTruncated" => Some(Self::Truncated),
            _ => None,
        }
    }
}

fn local_name(raw: &str) -> String {
    raw.rsplit(':').next().unwrap_or("").to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_listing_keeps_size_and_the_part_file() -> Result<(), String> {
        let xml = include_str!("../../../../fixtures/vision/listing_with_part.xml");
        let page = list_page(xml).map_err(|err| err.to_string())?;
        assert!(!page.truncated);
        assert_eq!(page.objects.len(), 3);
        assert_eq!(page.objects[0].size, 67872);
        assert!(page.objects[2].key.contains("part-00000"));
        Ok(())
    }
}
