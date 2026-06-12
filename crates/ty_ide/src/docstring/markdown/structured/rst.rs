use ruff_text_size::TextSize;
use rustc_hash::FxHashMap;

use super::{SectionBlock, SectionCandidate, SectionItem};
use crate::docstring::formats::{SectionKind, rst};

pub(super) fn section_candidates(docstring: &rst::Docstring) -> Vec<SectionCandidate> {
    let mut sections = Vec::new();

    for field_list in docstring.field_lists() {
        if field_list.indent() != TextSize::default() {
            continue;
        }

        let Some(section) = section_block(field_list) else {
            continue;
        };

        let range = field_list.range();
        sections.push(SectionCandidate {
            range: range.start().to_usize()..range.end().to_usize(),
            block: section,
        });
    }

    sections
}

fn section_block(field_list: &rst::FieldList) -> Option<SectionBlock> {
    let fields = field_list.fields();
    RenderPlan::from_fields(fields)?.execute(fields)
}

/// Validates a reST field list and stores cross-field metadata needed while rendering.
struct RenderPlan<'a> {
    parameter_types: SupplementalTypeFields<'a>,
    attribute_types: SupplementalTypeFields<'a>,
    return_type: Option<&'a str>,
    has_returns: bool,
}

impl<'a> RenderPlan<'a> {
    fn from_fields(fields: &'a [rst::Field]) -> Option<Self> {
        let mut has_returns = false;
        let mut parameter_types = SupplementalTypeFields::default();
        let mut attribute_types = SupplementalTypeFields::default();
        let mut return_type = None;

        for field in fields {
            match field {
                rst::Field::Parameter {
                    lookup_name, ty, ..
                } => {
                    parameter_types.record_value_field(lookup_name.as_str(), ty.is_some());
                }
                rst::Field::Attribute { name, ty, .. } => {
                    attribute_types.record_value_field(name.as_str(), ty.is_some());
                }
                rst::Field::Returns { .. } => {
                    has_returns = true;
                }
                rst::Field::Raises { .. } => {}
                rst::Field::ParameterType { lookup_name, ty } => {
                    parameter_types.record_type_field(lookup_name.as_str(), ty.as_str())?;
                }
                rst::Field::AttributeType { name, ty } => {
                    attribute_types.record_type_field(name.as_str(), ty.as_str())?;
                }
                rst::Field::ReturnType { ty } => {
                    if return_type.replace(ty.as_str()).is_some() {
                        return None;
                    }
                }
                rst::Field::Metadata => {
                    // Sphinx metadata fields are not user-facing hover sections.
                }
                rst::Field::Unknown { .. } => {
                    // Unknown or unsupported fields may have section semantics we
                    // do not understand, so leave the full field list unstructured.
                    return None;
                }
            }
        }

        if !parameter_types.all_types_match_value_fields() {
            return None;
        }

        if !attribute_types.all_types_match_value_fields() {
            return None;
        }

        Some(Self {
            parameter_types,
            attribute_types,
            return_type,
            has_returns,
        })
    }

    fn execute(&self, fields: &'a [rst::Field]) -> Option<SectionBlock> {
        let items = self.items(fields);

        if items.is_empty() || items.iter().any(SectionItem::is_empty) {
            return None;
        }

        Some(SectionBlock::new(items))
    }

    fn items(&self, fields: &'a [rst::Field]) -> Vec<SectionItem> {
        let mut items = Vec::new();

        for field in fields {
            match field {
                rst::Field::Parameter {
                    display_name,
                    lookup_name,
                    ty,
                    description,
                } => items.push(SectionItem::new(
                    SectionKind::Parameters,
                    Some(display_name.as_str()),
                    ty.as_deref()
                        .or_else(|| self.parameter_types.get_non_empty(lookup_name.as_str())),
                    description,
                )),
                rst::Field::Attribute {
                    name,
                    ty,
                    description,
                } => items.push(SectionItem::new(
                    SectionKind::Attributes,
                    Some(name.as_str()),
                    ty.as_deref()
                        .or_else(|| self.attribute_types.get_non_empty(name.as_str())),
                    description,
                )),
                rst::Field::Returns { name, description } => items.push(SectionItem::new(
                    SectionKind::Returns,
                    name.as_deref(),
                    self.return_type.filter(|ty| !ty.is_empty()),
                    description,
                )),
                rst::Field::Raises {
                    exception,
                    description,
                } => items.push(SectionItem::new(
                    SectionKind::Raises,
                    exception.as_deref(),
                    None,
                    description,
                )),
                rst::Field::ReturnType { .. } if !self.has_returns => {
                    if let Some(return_type) = self.return_type.filter(|ty| !ty.is_empty()) {
                        items.push(SectionItem::new(
                            SectionKind::Returns,
                            None,
                            Some(return_type),
                            "",
                        ));
                    }
                }
                rst::Field::ParameterType { .. }
                | rst::Field::AttributeType { .. }
                | rst::Field::ReturnType { .. }
                | rst::Field::Metadata
                | rst::Field::Unknown { .. } => {}
            }
        }

        items
    }
}

/// Tracks `:type name:` fields that supplement matching value fields.
///
/// A separate type field is usable only when the corresponding value field
/// exists and did not already include an inline type.
#[derive(Default)]
struct SupplementalTypeFields<'a> {
    types: FxHashMap<&'a str, &'a str>,
    value_fields_accepting_type: FxHashMap<&'a str, bool>,
}

impl<'a> SupplementalTypeFields<'a> {
    fn record_value_field(&mut self, name: &'a str, has_inline_type: bool) {
        self.value_fields_accepting_type
            .entry(name)
            .and_modify(|accepts_separate_type| *accepts_separate_type &= !has_inline_type)
            .or_insert(!has_inline_type);
    }

    fn record_type_field(&mut self, name: &'a str, ty: &'a str) -> Option<()> {
        self.types.insert(name, ty).is_none().then_some(())
    }

    fn all_types_match_value_fields(&self) -> bool {
        self.types.keys().all(|name| {
            self.value_fields_accepting_type
                .get(name)
                .copied()
                .unwrap_or(false)
        })
    }

    fn get_non_empty(&self, name: &str) -> Option<&'a str> {
        self.types.get(name).copied().filter(|ty| !ty.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use insta::{Settings, assert_snapshot};

    use super::super::Docstring;
    use crate::docstring::formats::Formats;

    #[test]
    fn rest_field_lists_render_markdown_sections() {
        let docstring = "\
Summary.

:param str value: The value.
:param other: Another value.
:type other: int
:returns: Whether validation passed.
:rtype: bool
";
        let parsed = parse_docstring(docstring);

        assert_snapshot!(parsed.render_markdown_source(), @"
        Summary.

        ## Parameters
        `value` (`str`): The value.
        `other` (`int`): Another value.

        ## Returns
        `bool`: Whether validation passed.
        ");

        let docstring = "\
:param value: Stale description.
:param value: Corrected description.
";
        let parsed = parse_docstring(docstring);

        assert_snapshot!(parsed.render_markdown_source(), @"
        ## Parameters
        `value`: Stale description.
        `value`: Corrected description.
        ");

        let docstring = "\
Summary.

:param value: The value.
:rtype: str
";
        let parsed = parse_docstring(docstring);

        assert_snapshot!(parsed.render_markdown_source(), @"
        Summary.

        ## Parameters
        `value`: The value.

        ## Returns
        `str`
        ");

        let docstring = "\
Summary.

:rtype: str
";
        let parsed = parse_docstring(docstring);

        assert_snapshot!(parsed.render_markdown_source(), @"
        Summary.

        ## Returns
        `str`
        ");
    }

    #[test]
    fn rest_field_lists_render_edge_cases() {
        let docstring = "\
This is a function description.
:class:`Foo` instances can be passed here.

:param str param1: The first parameter description
:meta private:
:param param2: The second parameter description
:type param2: int
:kwparam retries: Retry attempts.
:paramtype retries: int
:param *args: Extra positional arguments.
:type args: tuple[str, ...]
:param **kwargs: Extra keyword arguments.
:type **kwargs: dict[str, object]
:var cache: Cached data.
:vartype cache: dict[str,
    object]
:ivar state: Instance state.
:var str title: Display title.
:cvar VERSION: Package version.
:vartype VERSION: str
:returns baz: The return value description
:rtype: dict[str,
    int]
:raises ValueError: If the value is invalid.
:meta hide-value:
:exception RuntimeError: If the system is unavailable.";
        let parsed = parse_docstring(docstring);

        assert_snapshot!(parsed.render_markdown_source(), @"
        This is a function description.
        :class:`Foo` instances can be passed here.

        ## Parameters
        `param1` (`str`): The first parameter description
        `param2` (`int`): The second parameter description
        `retries` (`int`): Retry attempts.
        `*args` (`tuple[str, ...]`): Extra positional arguments.
        `**kwargs` (`dict[str, object]`): Extra keyword arguments.

        ## Attributes
        `cache` (`dict[str, object]`): Cached data.
        `state`: Instance state.
        `title` (`str`): Display title.
        `VERSION` (`str`): Package version.

        ## Returns
        `baz` (`dict[str, int]`): The return value description

        ## Raises
        `ValueError`: If the value is invalid.
        `RuntimeError`: If the system is unavailable.
        ");
    }

    #[test]
    fn rest_field_lists_preserve_unrenderable_and_preformatted_lists() {
        let docstring = "\
:param first: First parameter.
:type orphan: str

Some prose between field lists.

:meta private:

Markdown input:

```text
:param sample: This is sample input
```

Doctest output:

>>> print(\"field list\")
:param sample: This is sample output

Literal block::

    :param sample: This is sample input

:param quoted: Example::

:param sample: This is sample input
:returns: This is still sample input

:param second:
    - First option.
    - Second option.
:param third:
    1. Validate the input.
    2. Return the result.
:param done: Whether work is done.";
        let parsed = parse_docstring(docstring);
        let mut settings = Settings::clone_current();
        settings.add_filter("\n    \n", "\n<INDENTED-BLANK>\n");
        let _snap = settings.bind_to_scope();

        assert_snapshot!(parsed.render_markdown_source(), @"
        :param first: First parameter.
        :type orphan: str

        Some prose between field lists.

        :meta private:

        Markdown input:

        ```text
        :param sample: This is sample input
        ```

        Doctest output:

        >>> print(\"field list\")
        :param sample: This is sample output

        Literal block::

            :param sample: This is sample input

        ## Parameters
        `quoted`: Example::
        <INDENTED-BLANK>
            :param sample: This is sample input
            :returns: This is still sample input
        `second`:
        - First option.
        - Second option.

        `third`:
        1. Validate the input.
        2. Return the result.

        `done`: Whether work is done.
        ");
    }

    #[test]
    fn indented_sections_stay_raw() {
        let docstring = "\
Summary.

    :param value: The value.
    :returns: Another value.
";
        let parsed = parse_docstring(docstring);

        assert_eq!(parsed.render_markdown_source(), docstring);
    }

    #[test]
    fn unsupported_rest_field_lists_stay_raw() {
        for docstring in [
            "\
Summary.

:param value: The value.
:unknown field: Preserve this field list.
",
            "\
Summary.

:returns:
:raises:
",
            "\
Summary.

:param str value: The value.
:type value: int
",
            "\
Summary.

:param value: The value.
:type value: str
:type value: int
",
            "\
Summary.

:param value: The value.
:returns:
",
        ] {
            let parsed = parse_docstring(docstring);
            assert_eq!(parsed.render_markdown_source(), docstring);
        }
    }

    fn parse_docstring(raw: &str) -> Docstring<'_> {
        let formats = Formats::parse(raw);
        Docstring::parse(raw, &formats)
    }
}
