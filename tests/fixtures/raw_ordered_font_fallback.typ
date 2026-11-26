#set text(font: ("Libertinus Serif", "New Computer Modern"))

Body text before the example.
#text(font: ((name: "DejaVu Sans Mono", covers: regex("[0-9]")), "New Computer Modern"))[StyledProse].

#show raw: set text(font: (
  (name: "DejaVu Sans Mono", covers: regex("[0-9]")),
  "Zed Sans",
  "Libertinus Serif",
))

Inline `inlineToken` in a regular paragraph.

```python
def blockToken():
    return None
```
