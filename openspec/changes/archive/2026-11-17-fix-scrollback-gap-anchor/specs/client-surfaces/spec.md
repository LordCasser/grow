## ADDED Requirements

### Requirement: Scrollback gap anchors follow their preceding entry

Scrollback SHALL 在viewport顶端位于entry后的布局间隙时保留该间隙相对前一entry末尾的位置。历史前插或换宽重排SHALL NOT 将它夹到正文内容行；正文逻辑行内的锚点仍按新行高夹紧。

#### Scenario: A gap anchor roundtrips without a layout change
- **WHEN** viewport顶端落在一行entry后的gap，随后capture并在同几何restore
- **THEN** 顶端仍为相同gap行，不跳到前一内容行。

#### Scenario: A preceding entry changes geometry
- **WHEN** 补入旧历史或换宽使该entry的位置或高度变化
- **THEN** viewport锚到原entry之后的相同gap偏移，正文内锚点继续保持原逻辑行语义。
