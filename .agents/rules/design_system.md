# Eronom Design System (EDS) & LLM-Safe Component Guidelines

Follow these mandatory rules when creating or modifying UI components (`.erm`) in Eronom:

## 1. Core Philosophy: Intent over Arbitrary Values
- **Never Author in Values**: Do not write raw pixel padding (`p-4`, `p-[17px]`) or hardcoded hex colors (`#ffffff`, `#1a1a1a`). Use semantic intent tokens.
- **Closed Vocabulary**: The compiler enforces closed token enums. Off-system tokens produce hard compile errors.

## 2. Polymorphic Layout Primitives
- **Semantic Components**:
  - Use `<Box as="...">` for general containers (e.g. `<Box as="nav">`, `<Box as="section">`, `<Box as="main">`, `<Box as="ul">`, `<Box as="li">`).
  - Use `<Stack gap="...">` for vertical column layouts (`flex-direction: column`).
  - Use `<Cluster gap="..." align="...">` for horizontal wrapping rows (`flex-direction: row; flex-wrap: wrap`).
  - Use `<Text as="...">` for typography (`<Text as="h1">`, `<Text as="p">`, `<Text as="span">`).
- **Vanilla HTML Interoperability**: Standard HTML elements (such as `<div>`, `<span>`, `<section>`) and custom CSS classes remain fully supported.

## 3. Zero-Cost Native Dark Mode
- **Never Write `dark:` Modifiers**: All EDS tokens resolve via native browser CSS `light-dark()`.
- Writing `bg="card"` or `color="primary"` renders the correct value in both light and dark modes automatically.

## 4. Authorized Token Reference

### A. Spacing & Gaps (`p`, `px`, `py`, `gap`)
`"none"`, `"xs"` (4px), `"sm"` (8px), `"md"` (12px), `"lg"` (16px), `"xl"` (24px), `"2xl"` (32px), `"3xl"` (48px).

### B. Surface Backgrounds (`bg`)
`"canvas"`, `"card"`, `"muted"`, `"overlay"`, `"primary"`, `"danger"`, `"success"`, `"transparent"`.

### C. Typography (`<Text variant="..." color="...">`)
- **Variants**: `"display"`, `"title-lg"`, `"title-md"`, `"heading"`, `"body"`, `"body-sm"`, `"caption"`, `"mono"`.
- **Colors**: `"primary"`, `"secondary"`, `"muted"`, `"on-primary"`, `"danger"`, `"success"`.
- **Weights**: `"normal"`, `"medium"`, `"semibold"`, `"bold"`.

### D. Borders & Radii (`border`, `radius`)
- **Borders**: `"subtle"`, `"strong"`, `"focus"`, `"none"`.
- **Radii**: `"none"`, `"sm"` (4px), `"md"` (8px), `"lg"` (12px), `"xl"` (16px), `"full"` (9999px).

## 5. Composite Primitives
- `<Card>`: Pre-styled card surface container with padding, subtle border, radius, and soft shadow.
- `<Button variant="primary|secondary|subtle|danger" size="sm|md|lg">`: Interactive button.
- `<Badge status="default|success|warning|danger|info">`: Status badge pill.

## Example
```html
<Box as="main" p="xl" bg="canvas" width="full">
  <Stack gap="lg">
    <Card>
      <Text as="h1" variant="title-lg" weight="bold" color="primary">Analytics</Text>
      <Text variant="body" color="secondary">Overview of system health</Text>
      <Cluster gap="sm" align="center">
        <Badge status="success">Operational</Badge>
        <Button variant="primary" size="sm">Refresh</Button>
      </Cluster>
    </Card>
  </Stack>
</Box>
```
