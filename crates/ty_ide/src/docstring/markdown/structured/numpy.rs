use crate::docstring::formats::{
    self, SectionKind,
    numpy::{is_numpy_anonymous_return_type, is_numpy_item_name, split_numpy_type_separator},
};
use crate::docstring::parsing::ParsedLine;

use super::{
    DescriptionLine, SectionBlock, SectionCandidate, SectionItemBuilder, parse_named_items,
};

pub(super) fn section_candidates(
    docstring: &formats::numpy::Docstring<'_>,
) -> Vec<SectionCandidate> {
    docstring
        .sections()
        .iter()
        .filter_map(|section| {
            if section.indent() != 0 {
                return None;
            }
            let kind = section.kind();
            let block = numpy_section_block(kind, section.body())?;
            Some(SectionCandidate {
                range: section.range(),
                block,
            })
        })
        .collect()
}

fn numpy_section_block(kind: SectionKind, body: &[ParsedLine<'_>]) -> Option<SectionBlock> {
    let items = match kind {
        SectionKind::Parameters
        | SectionKind::KeywordArguments
        | SectionKind::OtherParameters
        | SectionKind::Attributes => parse_named_items(kind, body, parse_numpy_named_item)?,
        SectionKind::Returns | SectionKind::Yields => {
            parse_named_items(kind, body, parse_numpy_return_item)?
        }
        SectionKind::Raises => parse_named_items(kind, body, parse_numpy_raise_item)?,
    };

    Some(SectionBlock::new(items))
}

fn parse_numpy_named_item(line: &str) -> Option<SectionItemBuilder> {
    let (name, ty) = split_numpy_type_separator(line).map_or_else(
        || is_numpy_item_name(line.trim()).then_some((line.trim(), None)),
        |(name, ty)| Some((name, Some(ty))),
    )?;

    Some(SectionItemBuilder {
        display_name: Some(name.to_string()),
        ty: ty.map(str::to_string),
        description_lines: Vec::new(),
    })
}

fn parse_numpy_return_item(line: &str) -> Option<SectionItemBuilder> {
    parse_numpy_named_return_item(line).or_else(|| {
        is_numpy_anonymous_return_type(line).then(|| SectionItemBuilder {
            display_name: None,
            ty: Some(line.to_string()),
            description_lines: Vec::new(),
        })
    })
}

fn parse_numpy_named_return_item(line: &str) -> Option<SectionItemBuilder> {
    let (name, ty) = split_numpy_type_separator(line)?;

    Some(SectionItemBuilder {
        display_name: Some(name.to_string()),
        ty: Some(ty.to_string()),
        description_lines: Vec::new(),
    })
}

fn parse_numpy_raise_item(line: &str) -> Option<SectionItemBuilder> {
    let (name, description) = line
        .split_once(':')
        .map_or((line.trim(), None), |(name, description)| {
            (name.trim(), Some(description.trim()))
        });
    if !is_numpy_item_name(name) {
        return None;
    }

    Some(SectionItemBuilder {
        display_name: Some(name.to_string()),
        ty: None,
        description_lines: description
            .filter(|description| !description.is_empty())
            .map(DescriptionLine::normalized)
            .into_iter()
            .collect(),
    })
}
