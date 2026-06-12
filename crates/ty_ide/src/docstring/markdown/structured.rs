use std::borrow::Cow;
use std::ops::Range;

mod google;
mod rst;

use super::super::formats::{Formats, SectionKind};
use super::super::parsing::{ParsedLine, indentation};

const SECTION_ORDER: &[(SectionKind, &str)] = &[
    (SectionKind::Parameters, "Parameters"),
    (SectionKind::KeywordArguments, "Keyword Arguments"),
    (SectionKind::OtherParameters, "Other Parameters"),
    (SectionKind::Attributes, "Attributes"),
    (SectionKind::Returns, "Returns"),
    (SectionKind::Yields, "Yields"),
    (SectionKind::Raises, "Raises"),
];

fn render_markdown_section<'a>(
    output: &mut String,
    heading: &str,
    fields: impl Iterator<Item = &'a SectionItem>,
) {
    let mut previous_description = None;

    // Render each field into the output with the appropriate spacing between fields.
    for field in fields.filter(|field| !field.is_empty()) {
        if previous_description.is_none() {
            if !output.is_empty() {
                output.push_str("\n\n");
            }

            output.push_str("## ");
            output.push_str(heading);
            output.push('\n');
        }

        if let Some(description) = previous_description {
            render_separator_after_description(output, description);
        }

        field.render_into(output);
        previous_description = Some(field.description.as_str());
    }

    if let Some(description) = previous_description {
        render_section_end_after_description(output, description);
    }
}

fn render_inline_description(output: &mut String, description: &str) {
    if description.contains('\n') {
        output.push_str(&description.replace('\n', "\n    "));
    } else {
        output.push_str(description);
    }
}

fn render_separator_after_description(output: &mut String, description: &str) {
    let state = DescriptionState::scan(description);
    if let Some(fence) = state.open_markdown_fence() {
        output.push('\n');
        output.push_str(fence.marker());
        output.push_str("\n\n");
    } else if state.needs_blank_before_next_field() {
        // Add an extra newline to keep the next field out of an open block.
        output.push_str("\n\n");
    } else {
        output.push('\n');
    }
}

fn render_boundary_after_description(
    output: &mut String,
    description: &str,
    following_raw: Option<&str>,
) {
    let state = DescriptionState::scan(description);
    if state.open_markdown_fence().is_some() || state.needs_blank_before_next_field() {
        push_missing_blank_boundary(output, following_raw);
    } else if !following_raw.is_some_and(|raw| raw.starts_with('\n')) {
        output.push('\n');
    }
}

fn push_missing_blank_boundary(output: &mut String, following_raw: Option<&str>) {
    if following_raw.is_some_and(|raw| raw.starts_with("\n\n")) {
        return;
    }

    if following_raw.is_some_and(|raw| raw.starts_with('\n')) {
        output.push('\n');
    } else {
        output.push_str("\n\n");
    }
}

fn render_section_end_after_description(output: &mut String, description: &str) {
    let state = DescriptionState::scan(description);
    if let Some(fence) = state.open_markdown_fence() {
        output.push('\n');
        output.push_str(fence.marker());
    }
}

#[derive(Debug, Default)]
struct DescriptionState<'a> {
    markdown_fence: Option<super::MarkdownFence<'a>>,
    in_doctest: bool,
    // Markdown allows later paragraph lines to lazily continue a list item, so
    // any list item in the trailing block keeps the next field at risk.
    trailing_block_has_markdown_list: bool,
}

impl<'a> DescriptionState<'a> {
    fn scan(description: &'a str) -> Self {
        let mut state = Self::default();

        for line in description.lines().map(|line| line.trim_start_matches(' ')) {
            state.consume_line(line);
        }

        state
    }

    fn needs_blank_before_next_field(&self) -> bool {
        self.in_doctest || self.trailing_block_has_markdown_list
    }

    fn open_markdown_fence(&self) -> Option<super::MarkdownFence<'a>> {
        self.markdown_fence
    }

    fn consume_line(&mut self, line: &'a str) {
        if let Some(fence) = self.markdown_fence {
            if fence.is_closed_by(line) {
                self.markdown_fence = None;
            }
            return;
        }

        if self.in_doctest {
            if line.is_empty() {
                self.in_doctest = false;
                self.trailing_block_has_markdown_list = false;
            }
            return;
        }

        if line.is_empty() {
            self.trailing_block_has_markdown_list = false;
        } else if line.starts_with(">>>") {
            self.in_doctest = true;
            self.trailing_block_has_markdown_list = false;
        } else if let Some(fence) = super::MarkdownFence::find(line) {
            self.markdown_fence = Some(fence);
            self.trailing_block_has_markdown_list = false;
        } else if starts_with_markdown_list_item(line) {
            self.trailing_block_has_markdown_list = true;
        }
    }
}

fn description_block_start(description: &str) -> Option<usize> {
    let mut offset = 0;

    for line in description.split_inclusive('\n') {
        let line_without_newline = line.strip_suffix('\n').unwrap_or(line);
        if line_starts_block_content(line_without_newline) {
            return Some(offset);
        }

        offset += line.len();
    }

    None
}

fn line_starts_block_content(line: &str) -> bool {
    let line = line.trim_start_matches(' ');
    super::MarkdownFence::find(line).is_some()
        || line.starts_with(">>>")
        || starts_with_markdown_list_item(line)
}

fn starts_with_markdown_list_item(line: &str) -> bool {
    starts_with_unordered_markdown_list_item(line) || starts_with_ordered_markdown_list_item(line)
}

fn starts_with_unordered_markdown_list_item(line: &str) -> bool {
    matches!(
        line.as_bytes(),
        [b'-' | b'+' | b'*'] | [b'-' | b'+' | b'*', b' ' | b'\t', ..]
    )
}

fn starts_with_ordered_markdown_list_item(line: &str) -> bool {
    let bytes = line.as_bytes();
    let mut digit_count = 0;

    for byte in bytes {
        if digit_count < 9 && byte.is_ascii_digit() {
            digit_count += 1;
            continue;
        }

        if digit_count > 0 && matches!(*byte, b'.' | b')') {
            return bytes
                .get(digit_count + 1)
                .is_none_or(|byte| matches!(*byte, b' ' | b'\t'));
        }

        return false;
    }

    false
}

fn render_type_code_span_into(output: &mut String, ty: &str) {
    let normalized = normalize_type_for_code_span(ty);
    render_code_span_into(output, normalized.as_ref());
}

/// Normalizes type text so it fits in a single Markdown code span.
///
/// One-line types are returned unchanged. Multi-line types are trimmed line by
/// line, with empty lines discarded and remaining lines joined by a single
/// space.
///
/// For example:
///
/// ```python
/// dict[str,
///     object]
/// ```
///
/// becomes `dict[str, object]`.
fn normalize_type_for_code_span(ty: &str) -> Cow<'_, str> {
    if !ty.contains('\n') {
        return Cow::Borrowed(ty);
    }

    let mut normalized = String::new();
    for line in ty.lines().map(str::trim).filter(|line| !line.is_empty()) {
        if !normalized.is_empty() {
            normalized.push(' ');
        }
        normalized.push_str(line);
    }

    Cow::Owned(normalized)
}

/// Wraps `text` in a Markdown code span and appends it to output.
fn render_code_span_into(output: &mut String, text: &str) {
    // This chooses the number of backticks that we use to delimit the start and
    // end of the inline Markdown code span.
    //
    // The number we pick is one greater than the longest run of consecutive
    // backticks in `text`, which guarantees that we can wrap `text` unambiguously.
    let delimiter_len = text
        .split(|char| char != '`')
        .map(str::len)
        .max()
        .unwrap_or(0)
        + 1;

    output.extend(std::iter::repeat_n('`', delimiter_len));
    if text.starts_with('`') || text.ends_with('`') {
        // Per the CommonMark spec, wrap the contents of the code span in
        // whitespace if those contents start or end with backticks.
        //
        // <https://spec.commonmark.org/0.31.2/#code-spans>
        output.push(' ');
        output.push_str(text);
        output.push(' ');
    } else {
        output.push_str(text);
    }
    output.extend(std::iter::repeat_n('`', delimiter_len));
}

#[cfg(test)]
mod section_tests {
    use insta::{Settings, assert_snapshot};

    use super::{SectionBlock, SectionItem};
    use crate::docstring::formats::SectionKind;

    #[test]
    fn sections_render_in_canonical_order() {
        let section = SectionBlock::new(vec![
            SectionItem::new(
                SectionKind::Raises,
                Some("ValueError"),
                None,
                "Invalid value.",
            ),
            SectionItem::new(
                SectionKind::Parameters,
                Some("value"),
                Some("str"),
                "The value.",
            ),
            SectionItem::new(
                SectionKind::KeywordArguments,
                Some("limit"),
                Some("int"),
                "Maximum result count.",
            ),
            SectionItem::new(
                SectionKind::OtherParameters,
                Some("kw_only"),
                Some("str"),
                "Less common option.",
            ),
            SectionItem::new(
                SectionKind::Returns,
                None,
                Some("bool"),
                "Whether validation passed.",
            ),
            SectionItem::new(
                SectionKind::Attributes,
                Some("cache"),
                Some("dict[str,\n object]"),
                "Cached data.",
            ),
        ]);

        assert_snapshot!(section.render_markdown(), @"
        ## Parameters
        `value` (`str`): The value.

        ## Keyword Arguments
        `limit` (`int`): Maximum result count.

        ## Other Parameters
        `kw_only` (`str`): Less common option.

        ## Attributes
        `cache` (`dict[str, object]`): Cached data.

        ## Returns
        `bool`: Whether validation passed.

        ## Raises
        `ValueError`: Invalid value.
        ");
    }

    #[test]
    fn sections_skip_empty_items() {
        let section = SectionBlock::new(vec![
            SectionItem::new(SectionKind::Parameters, None, None, ""),
            SectionItem::new(SectionKind::Returns, None, Some(""), ""),
        ]);

        assert_eq!(section.render_markdown(), "");
    }

    #[test]
    fn sections_render_multiline_and_block_descriptions() {
        let mut settings = Settings::clone_current();
        settings.add_filter("\n    \n", "\n<INDENTED-BLANK>\n");
        let _snap = settings.bind_to_scope();

        let section = SectionBlock::new(vec![
            SectionItem::new(
                SectionKind::Parameters,
                Some("`value`"),
                None,
                "First sentence.\nContinued sentence.\n\nSecond paragraph.",
            ),
            SectionItem::new(
                SectionKind::Parameters,
                Some("mode"),
                None,
                "Allowed values:\n- fast\n- slow",
            ),
            SectionItem::new(
                SectionKind::Parameters,
                Some("example"),
                None,
                "Example:\n```python\nif ok:\n    do_work()\n```",
            ),
            SectionItem::new(
                SectionKind::Parameters,
                Some("prompt"),
                None,
                "Example:\n>>> print('prompt')",
            ),
            SectionItem::new(
                SectionKind::Parameters,
                Some("choices"),
                None,
                "- first\n- second",
            ),
            SectionItem::new(
                SectionKind::Parameters,
                Some("steps"),
                None,
                "1. first\n2. second",
            ),
            SectionItem::new(
                SectionKind::Parameters,
                Some("unterminated"),
                None,
                "```python\nprint('open')",
            ),
            SectionItem::new(
                SectionKind::Parameters,
                Some("other"),
                None,
                "Another parameter.",
            ),
            SectionItem::new(
                SectionKind::Returns,
                None,
                Some("str"),
                "```python\nprint('result')",
            ),
            SectionItem::new(
                SectionKind::Raises,
                Some("ValueError"),
                None,
                "Invalid value.",
            ),
        ]);

        assert_snapshot!(section.render_markdown(), @r#"
        ## Parameters
        `` `value` ``: First sentence.
            Continued sentence.
        <INDENTED-BLANK>
            Second paragraph.
        `mode`: Allowed values:

        - fast
        - slow

        `example`: Example:

        ```python
        if ok:
            do_work()
        ```
        `prompt`: Example:

        >>> print('prompt')

        `choices`:
        - first
        - second

        `steps`:
        1. first
        2. second

        `unterminated`:
        ```python
        print('open')
        ```

        `other`: Another parameter.

        ## Returns
        `str`:
        ```python
        print('result')
        ```

        ## Raises
        `ValueError`: Invalid value.
        "#);
    }
}

pub(super) fn render<'a>(raw: &'a str, formats: &Formats<'_>) -> Cow<'a, str> {
    Docstring::parse(raw, formats).render_markdown_source()
}

/// A tolerant, display-oriented parse of a normalized docstring.
pub(super) struct Docstring<'a> {
    raw: &'a str,
    blocks: Vec<Block<'a>>,
}

impl<'a> Docstring<'a> {
    pub(super) fn parse(raw: &'a str, formats: &Formats<'_>) -> Self {
        let blocks = parse_blocks(raw, formats);

        Self { raw, blocks }
    }

    pub(super) fn render_markdown_source(&self) -> Cow<'a, str> {
        if self.blocks.is_empty()
            || matches!(
                self.blocks.as_slice(),
                [Block::Raw(raw)] if *raw == self.raw
            )
        {
            return Cow::Borrowed(self.raw);
        }

        let mut output = String::new();
        for (index, block) in self.blocks.iter().enumerate() {
            match block {
                Block::Raw(raw) => output.push_str(raw),
                Block::Section(section) => {
                    output.push_str(&section.render_markdown());
                    if let Some(next) = self.blocks.get(index + 1) {
                        section.render_boundary_before_following_block(&mut output, next.as_raw());
                    }
                }
            }
        }

        Cow::Owned(output)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Block<'a> {
    Raw(&'a str),
    Section(SectionBlock),
}

impl Block<'_> {
    fn as_raw(&self) -> Option<&str> {
        match self {
            Self::Raw(raw) => Some(raw),
            Self::Section(_) => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SectionBlock {
    items: Vec<SectionItem>,
}

impl SectionBlock {
    pub(super) fn new(items: Vec<SectionItem>) -> Self {
        Self { items }
    }

    fn render_markdown(&self) -> String {
        let mut output = String::new();
        for &(kind, heading) in SECTION_ORDER {
            self.render_section(&mut output, heading, kind);
        }
        output
    }

    fn render_boundary_before_following_block(
        &self,
        output: &mut String,
        following_raw: Option<&str>,
    ) {
        if let Some(description) = self.last_rendered_description() {
            render_boundary_after_description(output, description, following_raw);
        }
    }

    fn render_section(&self, output: &mut String, heading: &str, kind: SectionKind) {
        render_markdown_section(
            output,
            heading,
            self.items.iter().filter(move |item| item.kind == kind),
        );
    }

    fn last_rendered_description(&self) -> Option<&str> {
        SECTION_ORDER.iter().rev().find_map(|(kind, _)| {
            self.items
                .iter()
                .rev()
                .find(|item| item.kind == *kind && !item.is_empty())
                .map(|item| item.description.as_str())
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SectionItem {
    kind: SectionKind,
    display_name: Option<String>,
    ty: Option<String>,
    description: String,
}

impl SectionItem {
    pub(super) fn new(
        kind: SectionKind,
        display_name: Option<&str>,
        ty: Option<&str>,
        description: &str,
    ) -> Self {
        Self {
            kind,
            display_name: display_name.map(str::to_string),
            ty: ty.map(str::to_string),
            description: description.to_string(),
        }
    }

    fn is_empty(&self) -> bool {
        self.display_name.is_none()
            && self.ty.as_deref().is_none_or(str::is_empty)
            && self.description.is_empty()
    }

    fn render_into(&self, output: &mut String) {
        let mut has_label = false;

        if let Some(name) = self.display_name.as_deref() {
            render_code_span_into(output, name);
            has_label = true;
        }

        if let Some(ty) = self.ty.as_deref()
            && !ty.is_empty()
        {
            if has_label {
                output.push_str(" (");
                render_type_code_span_into(output, ty);
                output.push(')');
            } else {
                render_type_code_span_into(output, ty);
                has_label = true;
            }
        }

        if !self.description.is_empty() {
            let description = self.description.as_str();
            let block_start = description_block_start(description);

            if has_label {
                output.push_str(if block_start == Some(0) { ":\n" } else { ": " });
            }

            if block_start == Some(0) {
                output.push_str(description);
            } else if let Some(block_start) = block_start {
                let before_block = &description[..block_start];
                render_inline_description(output, before_block.trim_end_matches('\n'));
                output.push_str("\n\n");
                output.push_str(&description[block_start..]);
            } else if description.contains('\n') {
                render_inline_description(output, description);
            } else {
                output.push_str(description);
            }
        }
    }
}

fn parse_blocks<'a>(raw: &'a str, formats: &Formats<'_>) -> Vec<Block<'a>> {
    let mut sections = rst::section_candidates(formats.rst());
    sections.extend(google::section_candidates(formats.google()));
    sections.sort_by_key(|section| section.range.start);
    let mut blocks = Vec::new();
    let mut rendered_through = 0;

    for section in sections {
        let start = section.range.start;
        let end = section.range.end;

        if start < rendered_through {
            continue;
        }

        if !push_raw_block(&mut blocks, raw, rendered_through..start) {
            return Vec::new();
        }
        rendered_through = end;
        blocks.push(Block::Section(section.block));
    }

    if !blocks.is_empty() && !push_raw_block(&mut blocks, raw, rendered_through..raw.len()) {
        return Vec::new();
    }

    blocks
}

fn push_raw_block<'a>(blocks: &mut Vec<Block<'a>>, raw: &'a str, range: Range<usize>) -> bool {
    if range.is_empty() {
        return true;
    }

    let Some(raw) = raw.get(range) else {
        return false;
    };
    blocks.push(Block::Raw(raw));
    true
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SectionCandidate {
    range: Range<usize>,
    block: SectionBlock,
}

pub(super) struct SectionItemBuilder {
    display_name: Option<String>,
    ty: Option<String>,
    description_lines: Vec<DescriptionLine>,
}

impl SectionItemBuilder {
    pub(super) fn finish(self, kind: SectionKind) -> SectionItem {
        let description = normalize_description(self.description_lines);
        SectionItem::new(
            kind,
            self.display_name.as_deref(),
            self.ty.as_deref(),
            &description,
        )
    }

    pub(super) fn push_description(&mut self, line: &str) {
        self.description_lines
            .push(DescriptionLine::Source(line.to_string()));
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum DescriptionLine {
    Normalized(String),
    Source(String),
}

impl DescriptionLine {
    pub(super) fn normalized(line: &str) -> Self {
        Self::Normalized(line.trim().to_string())
    }
}

pub(super) fn parse_named_items(
    kind: SectionKind,
    body: &[ParsedLine<'_>],
    mut parse_item: impl FnMut(&str) -> Option<SectionItemBuilder>,
) -> Option<Vec<SectionItem>> {
    let mut items = Vec::new();
    let mut current: Option<SectionItemBuilder> = None;
    let mut item_indent = None;

    for line in body {
        let trimmed = line.text.trim();
        if trimmed.is_empty() {
            if let Some(current) = &mut current {
                current.push_description("");
            }
            continue;
        }

        let line_indent = indentation(line.text);
        if item_indent.is_none_or(|indent| line_indent == indent) {
            if let Some(item) = parse_item(trimmed) {
                if let Some(current) = current.replace(item) {
                    items.push(current.finish(kind));
                }
                item_indent.get_or_insert(line_indent);
                continue;
            }
            if item_indent.is_some() {
                return None;
            }
        }
        if item_indent.is_some_and(|indent| line_indent < indent) {
            return None;
        }

        let current = current.as_mut()?;
        current.push_description(line.text);
    }

    if let Some(current) = current {
        items.push(current.finish(kind));
    }
    (!items.is_empty()).then_some(items)
}

pub(super) fn is_uri_scheme_prefix(ty: &str, description: &str) -> bool {
    if !is_uri_scheme(ty) {
        return false;
    }

    if description.starts_with("//") {
        return true;
    }

    let Some(first) = description.chars().next() else {
        return false;
    };
    if first.is_whitespace() {
        return false;
    }

    matches!(first, '/' | '?' | '#' | '@' | ':')
        || description
            .chars()
            .skip(1)
            .any(|char| matches!(char, '/' | '?' | '#' | '@' | ':'))
}

pub(super) fn is_uri_scheme(scheme: &str) -> bool {
    let mut chars = scheme.chars();
    chars.next().is_some_and(|char| char.is_ascii_alphabetic())
        && chars.all(|char| char.is_ascii_alphanumeric() || matches!(char, '+' | '-' | '.'))
}

pub(super) fn normalize_description(lines: Vec<DescriptionLine>) -> String {
    let dedent = lines
        .iter()
        .filter_map(|line| match line {
            DescriptionLine::Source(line) if !line.trim().is_empty() => Some(indentation(line)),
            DescriptionLine::Normalized(_) | DescriptionLine::Source(_) => None,
        })
        .min()
        .unwrap_or(0);

    let mut description = lines
        .into_iter()
        .map(|line| match line {
            DescriptionLine::Normalized(line) => line,
            DescriptionLine::Source(line) => {
                strip_indentation(&line, dedent).trim_end().to_string()
            }
        })
        .skip_while(String::is_empty)
        .collect::<Vec<_>>();
    while description.last().is_some_and(String::is_empty) {
        description.pop();
    }
    description.join("\n")
}

pub(super) fn strip_indentation(line: &str, width: usize) -> &str {
    let mut indentation_width = 0;
    for (index, char) in line.char_indices() {
        let char_width = match char {
            ' ' => 1,
            '\t' => 8,
            _ => return &line[index..],
        };

        if indentation_width + char_width > width {
            return &line[index..];
        }

        indentation_width += char_width;
        if indentation_width == width {
            return &line[index + char.len_utf8()..];
        }
    }

    ""
}

#[cfg(test)]
mod tests {
    use insta::assert_snapshot;

    use super::{Block, Docstring, SectionBlock, SectionItem};
    use crate::docstring::formats::{Formats, SectionKind};

    #[test]
    fn raw_docstring_renders_borrowed() {
        let docstring = "Summary.\n\nDetails.";
        let formats = Formats::parse(docstring);
        let parsed = Docstring::parse(docstring, &formats);

        assert_eq!(parsed.render_markdown_source(), docstring);

        let parsed = Docstring {
            raw: docstring,
            blocks: vec![Block::Raw(&docstring[.."Summary.".len()])],
        };

        assert_eq!(parsed.render_markdown_source(), "Summary.");
    }

    #[test]
    fn google_sections_render_markdown_sections() {
        let docstring = "\
Summary.

Args:
    value (str): The value.
        More detail.
    *items: Extra items.

Keyword Args:
    optional (int): Optional value.

Returns:
    bool: Whether validation passed.

Yields:
    int: Next value.
";
        let parsed = parse_docstring(docstring);

        assert_snapshot!(parsed.render_markdown_source(), @"
        Summary.

        ## Parameters
        `value` (`str`): The value.
            More detail.
        `*items`: Extra items.

        ## Keyword Arguments
        `optional` (`int`): Optional value.

        ## Returns
        `bool`: Whether validation passed.

        ## Yields
        `int`: Next value.
        ");

        let docstring = "\
Args:
    x, y: Coordinates.
";
        let parsed = parse_docstring(docstring);

        assert_snapshot!(parsed.render_markdown_source(), @"
        ## Parameters
        `x, y`: Coordinates.
        ");

        let docstring = "\
Keyword Arguments:
    retries: Retry count.
";
        let parsed = parse_docstring(docstring);

        assert_snapshot!(parsed.render_markdown_source(), @"
        ## Keyword Arguments
        `retries`: Retry count.
        ");

        let docstring = "\
Args:
    value: The value.
Additional details.
";
        let parsed = parse_docstring(docstring);

        assert_snapshot!(parsed.render_markdown_source(), @"
        ## Parameters
        `value`: The value.
        Additional details.
        ");

        let docstring = "\
Args:
    value: The value.
Methods:
    work: Does work.
";
        let parsed = parse_docstring(docstring);

        assert_snapshot!(parsed.render_markdown_source(), @"
        ## Parameters
        `value`: The value.
        Methods:
            work: Does work.
        ");

        let docstring = "\
Returns:
    bool: Whether validation passed.
Additional details.
";
        let parsed = parse_docstring(docstring);

        assert_snapshot!(parsed.render_markdown_source(), @"
        ## Returns
        `bool`: Whether validation passed.
        Additional details.
        ");

        let docstring = "\
Returns:
    str or None: Optional value.
";
        let parsed = parse_docstring(docstring);

        assert_snapshot!(parsed.render_markdown_source(), @"
        ## Returns
        `str or None`: Optional value.
        ");

        let docstring = "\
Yields:
    :obj:`list` of :obj:`str`: Result chunks.
";
        let parsed = parse_docstring(docstring);

        assert_snapshot!(parsed.render_markdown_source(), @"
        ## Yields
        `` :obj:`list` of :obj:`str` ``: Result chunks.
        ");

        let docstring = "\
Returns:
    str: Example output.
        ```python
        Args:
            value: still code.
        Returns:
            still code.
        ```
";
        let parsed = parse_docstring(docstring);

        assert_snapshot!(parsed.render_markdown_source(), @"
        ## Returns
        `str`: Example output.

        ```python
        Args:
            value: still code.
        Returns:
            still code.
        ```
        ");

        let docstring = "\
Yields:
    int: Example output.
        Example::
            Args:
                still code.
            Yields:
                still code.
";
        let parsed = parse_docstring(docstring);

        assert_snapshot!(parsed.render_markdown_source(), @"
        ## Yields
        `int`: Example output.
            Example::
                Args:
                    still code.
                Yields:
                    still code.
        ");
    }

    #[test]
    fn unsupported_google_sections_stay_raw() {
        let docstring = "\
Summary.

Args:
    Inputs are normalized first.
    value: The value.
";
        let parsed = parse_docstring(docstring);

        assert_eq!(parsed.render_markdown_source(), docstring);

        let docstring = "\
Summary.

Examples:
    Args:
        value: demo input.
";
        let parsed = parse_docstring(docstring);

        assert_eq!(parsed.render_markdown_source(), docstring);

        let docstring = "\
Summary.

Returns:
    bool: Whether validation passed.

    Examples:
        Use it.
";
        let parsed = parse_docstring(docstring);

        assert_eq!(parsed.render_markdown_source(), docstring);

        let docstring = "\
Summary.

Yields:
    int: Next value.

    Examples:
        Use it.
";
        let parsed = parse_docstring(docstring);

        assert_eq!(parsed.render_markdown_source(), docstring);

        let docstring = "\
Summary.

Args:
    Inputs are normalized first.
    Args:
        value: demo input.
";
        let parsed = parse_docstring(docstring);

        assert_eq!(parsed.render_markdown_source(), docstring);

        let docstring = "\
Summary.

Args:
    value: The value.

    Examples:
        Use it.
";
        let parsed = parse_docstring(docstring);

        assert_eq!(parsed.render_markdown_source(), docstring);

        let docstring = "\
Summary.

Returns:
    Examples:
        Use it.
";
        let parsed = parse_docstring(docstring);

        assert_eq!(parsed.render_markdown_source(), docstring);

        let docstring = "\
Summary.

Args:
    value: Example.
        ```python

Args:
    nested = 1
        ```
";
        let parsed = parse_docstring(docstring);

        assert_eq!(parsed.render_markdown_source(), docstring);
    }

    #[test]
    fn section_blocks_render_markdown_source() {
        let parsed = Docstring {
            raw: "Summary.\n\n:param str value: The value.",
            blocks: vec![
                Block::Raw("Summary.\n\n"),
                Block::Section(SectionBlock::new(vec![
                    SectionItem::new(
                        SectionKind::Parameters,
                        Some("value"),
                        Some("str"),
                        "The value.",
                    ),
                    SectionItem::new(
                        SectionKind::Returns,
                        None,
                        Some("bool"),
                        "Whether validation passed.",
                    ),
                ])),
            ],
        };

        assert_snapshot!(parsed.render_markdown_source(), @"
        Summary.

        ## Parameters
        `value` (`str`): The value.

        ## Returns
        `bool`: Whether validation passed.
        ");
    }

    #[test]
    fn section_blocks_separate_following_raw_blocks() {
        let parsed = Docstring {
            raw: ":param value: The value.\nAfter.",
            blocks: vec![
                Block::Section(SectionBlock::new(vec![SectionItem::new(
                    SectionKind::Parameters,
                    Some("value"),
                    None,
                    "The value.",
                )])),
                Block::Raw("After."),
            ],
        };

        assert_snapshot!(parsed.render_markdown_source(), @"
        ## Parameters
        `value`: The value.
        After.
        ");

        let parsed = Docstring {
            raw: ":param value: The value.\n\nAfter.",
            blocks: vec![
                Block::Section(SectionBlock::new(vec![SectionItem::new(
                    SectionKind::Parameters,
                    Some("value"),
                    None,
                    "The value.",
                )])),
                Block::Raw("\n\nAfter."),
            ],
        };

        assert_snapshot!(parsed.render_markdown_source(), @"
        ## Parameters
        `value`: The value.

        After.
        ");

        let parsed = Docstring {
            raw: ":param value:\n    - First option.\nAfter.",
            blocks: vec![
                Block::Section(SectionBlock::new(vec![SectionItem::new(
                    SectionKind::Parameters,
                    Some("value"),
                    None,
                    "- First option.",
                )])),
                Block::Raw("After."),
            ],
        };

        assert_snapshot!(parsed.render_markdown_source(), @"
        ## Parameters
        `value`:
        - First option.

        After.
        ");

        let parsed = Docstring {
            raw: ":param value:\n    - First option.\nAfter.",
            blocks: vec![
                Block::Section(SectionBlock::new(vec![SectionItem::new(
                    SectionKind::Parameters,
                    Some("value"),
                    None,
                    "- First option.",
                )])),
                Block::Raw("\nAfter."),
            ],
        };

        assert_snapshot!(parsed.render_markdown_source(), @"
        ## Parameters
        `value`:
        - First option.

        After.
        ");

        let parsed = Docstring {
            raw: ":param value:\n    ```python\n    value = 1\nAfter.",
            blocks: vec![
                Block::Section(SectionBlock::new(vec![SectionItem::new(
                    SectionKind::Parameters,
                    Some("value"),
                    None,
                    "```python\nvalue = 1",
                )])),
                Block::Raw("After."),
            ],
        };

        assert_snapshot!(parsed.render_markdown_source(), @"
        ## Parameters
        `value`:
        ```python
        value = 1
        ```

        After.
        ");

        let parsed = Docstring {
            raw: "Yields:\n    int:\n        - Next value.\nAfter.",
            blocks: vec![
                Block::Section(SectionBlock::new(vec![SectionItem::new(
                    SectionKind::Yields,
                    None,
                    Some("int"),
                    "- Next value.",
                )])),
                Block::Raw("After."),
            ],
        };

        assert_snapshot!(parsed.render_markdown_source(), @"
        ## Yields
        `int`:
        - Next value.

        After.
        ");
    }

    fn parse_docstring(raw: &str) -> Docstring<'_> {
        let formats = Formats::parse(raw);
        Docstring::parse(raw, &formats)
    }
}
