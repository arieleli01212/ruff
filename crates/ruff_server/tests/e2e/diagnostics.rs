use anyhow::Result;
use insta::assert_json_snapshot;

use crate::TestServerBuilder;

#[test]
fn related_information() -> Result<()> {
    let source = "{1, 1}\n";
    let mut server = TestServerBuilder::new()?
        .with_workspace(".")?
        .with_file(
            "pyproject.toml",
            r#"
[tool.ruff.lint]
select = ["B033"]
"#,
        )?
        .with_file("test.py", source)?
        .enable_diagnostic_related_information(true)
        .build();

    server.open_text_document("test.py", source, 1);
    let diagnostics = server.document_diagnostic_request("test.py", None);

    assert_json_snapshot!(diagnostics, @r#"
    {
      "items": [
        {
          "range": {
            "start": {
              "line": 0,
              "character": 4
            },
            "end": {
              "line": 0,
              "character": 5
            }
          },
          "severity": 2,
          "code": "B033",
          "codeDescription": {
            "href": "https://docs.astral.sh/ruff/rules/duplicate-value"
          },
          "source": "Ruff",
          "message": "Sets should not contain duplicate item `1`\n\nhelp: Remove duplicate item",
          "tags": [],
          "relatedInformation": [
            {
              "location": {
                "uri": "file://<temp_dir>/test.py",
                "range": {
                  "start": {
                    "line": 0,
                    "character": 1
                  },
                  "end": {
                    "line": 0,
                    "character": 2
                  }
                }
              },
              "message": "Previous occurrence here"
            }
          ],
          "data": {
            "code": "B033",
            "edits": [
              {
                "newText": "",
                "range": {
                  "end": {
                    "character": 5,
                    "line": 0
                  },
                  "start": {
                    "character": 2,
                    "line": 0
                  }
                }
              }
            ],
            "noqa_edit": {
              "newText": "  # noqa: B033\n",
              "range": {
                "end": {
                  "character": 0,
                  "line": 1
                },
                "start": {
                  "character": 6,
                  "line": 0
                }
              }
            },
            "title": "Remove duplicate item"
          }
        }
      ],
      "kind": "full"
    }
    "#);

    Ok(())
}
