# Customizing the Grow Logo

Grow includes its character-art logo in the binary, so the logo is available
even on a clean installation. You can replace either built-in size with a
plain-text file in your Grow home directory. This is a display customization,
not a configuration setting.

## Logo directory and files

The default Grow home is `~/.grow`. The logo override directory is:

```text
$GROW_HOME/logo/
```

When `GROW_HOME` is not set, this is `~/.grow/logo/`. The directory contains
two optional files:

| File | Intended size | Typical use |
|------|---------------|-------------|
| `big.txt` | 80 columns × 35 rows | Large welcome and empty-session layouts |
| `small.txt` | 50 columns × 22 rows | Compact welcome and empty-session layouts |

The files must be UTF-8 plain text. Do not include ANSI escape sequences,
terminal control codes, or a surrounding code fence. Grow measures width in
terminal display columns (not bytes or Unicode scalar count), and height in
effective lines. Each logo file may be at most 1 MiB; if one file exceeds that
limit, only that slot falls back to its compiled-in logo. Keep the artwork
within the recommended dimensions so it can be shown without clipping.

Each file is an independent override. You may provide only `big.txt` or only
`small.txt`; the missing slot falls back to the corresponding compiled-in
logo. A custom file replaces the whole slot rather than being merged with the
built-in artwork.

## Installing a logo

Create the directory, then copy your prepared files into it:

```sh
mkdir -p "${GROW_HOME:-$HOME/.grow}/logo"
cp path/to/my-big-logo.txt "${GROW_HOME:-$HOME/.grow}/logo/big.txt"
cp path/to/my-small-logo.txt "${GROW_HOME:-$HOME/.grow}/logo/small.txt"
```

If you want to edit a slot directly, open the target file with your preferred
editor after creating the directory:

```sh
mkdir -p "${GROW_HOME:-$HOME/.grow}/logo"
"${EDITOR:-vi}" "${GROW_HOME:-$HOME/.grow}/logo/big.txt"
```

The same process applies to `small.txt`. The editor must save UTF-8 text.
Restart Grow after changing either file; logo files are loaded at startup and
are not hot-reloaded during a running process.

## Where the logo appears

Grow chooses the slot that fits the available terminal area. The welcome page
uses `big.txt` or `small.txt` responsively. The project trust and startup gate
pages use the largest slot that fits their stacked layout. Once a session has
started, a session with no scrollback messages shows the logo centered as a
bare empty-state view, again selecting `big.txt` or `small.txt` as space
allows. As soon as a message appears, the empty-state logo is removed.

If the terminal is too small for either slot, Grow does not draw the logo. It
does not crop or partially render character art.

The current built-in layout measurements are useful targets when designing a
replacement. They describe the available layout, including surrounding UI,
not only the logo file itself:

| Display scene | Layout | Reference width × height |
|---------------|--------|--------------------------|
| Welcome | Side-by-side, `big.txt` | 143 × 39 |
| Welcome | Side-by-side, `small.txt` | 113 × 26 |
| Welcome | Stacked, `small.txt` | 54 × 26 |
| Project trust / startup gate | Stacked, `big.txt` | 84 × 42 |
| Project trust / startup gate | Stacked, `small.txt` | 54 × 29 |
| Session empty state | Centered, `big.txt` | 84 × 39 |
| Session empty state | Centered, `small.txt` | 54 × 26 |

These thresholds are recalculated from the actual custom artwork dimensions.
The table therefore describes the built-in artwork and surrounding layout,
not fixed terminal breakpoints. Reusing the built-in `80 × 35` and `50 × 22`
slot dimensions gives the most consistent behavior across all scenes.

## Troubleshooting

- Confirm the file is exactly named `big.txt` or `small.txt` under the active
  `$GROW_HOME/logo/` directory.
- Check the active Grow home if `GROW_HOME` is set; it takes the place of
  `~/.grow`.
- Remove or rename an override file to restore that slot's compiled-in logo.
- If the logo is absent, enlarge the terminal window. Grow intentionally hides
  artwork when the available area cannot contain it.
