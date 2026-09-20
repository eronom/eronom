// eronom-extension/src/edsData.ts - Eronom Design System (EDS) Metadata

export interface EdTokenValue {
  name: string;
  description: string;
  cssOutput?: string;
}

export interface EdPropInfo {
  name: string;
  type: string;
  description: string;
  values?: EdTokenValue[];
  isBoolean?: boolean;
}

export interface EdPrimitiveInfo {
  name: string;
  description: string;
  defaultTag: string;
  snippet: string;
  props: Record<string, EdPropInfo>;
}

// Spacing Tokens
export const SPACING_TOKENS: EdTokenValue[] = [
  { name: 'none', description: 'Zero spacing (0px)', cssOutput: '0' },
  { name: 'xs', description: 'Extra small spacing (4px / 0.25rem)', cssOutput: '0.25rem' },
  { name: 'sm', description: 'Small spacing (8px / 0.5rem)', cssOutput: '0.5rem' },
  { name: 'md', description: 'Medium spacing (12px / 0.75rem)', cssOutput: '0.75rem' },
  { name: 'lg', description: 'Large spacing (16px / 1rem)', cssOutput: '1rem' },
  { name: 'xl', description: 'Extra large spacing (24px / 1.5rem)', cssOutput: '1.5rem' },
  { name: '2xl', description: '2X large spacing (32px / 2rem)', cssOutput: '2rem' },
  { name: '3xl', description: '3X large spacing (48px / 3rem)', cssOutput: '3rem' },
  { name: 'auto', description: 'Automatic spacing (margin: auto)', cssOutput: 'auto' },
];

// Surface / Background Colors
export const SURFACE_COLOR_TOKENS: EdTokenValue[] = [
  { name: 'canvas', description: 'Main canvas/page background with automatic dual-theme light-dark() resolution', cssOutput: 'light-dark(#ffffff, #09090b)' },
  { name: 'body', description: 'Base body background with dual-theme resolution', cssOutput: 'light-dark(#ffffff, #09090b)' },
  { name: 'card', description: 'Elevated card surface background with subtle contrast', cssOutput: 'light-dark(#fafafa, #121215)' },
  { name: 'muted', description: 'Muted surface background for secondary containers', cssOutput: 'light-dark(#f4f4f5, #18181b)' },
  { name: 'overlay', description: 'Elevated overlay background (popovers, dialogs)', cssOutput: 'light-dark(#ffffff, #18181b)' },
  { name: 'primary', description: 'Brand primary accent background', cssOutput: 'light-dark(#2563eb, #3b82f6)' },
  { name: 'danger', description: 'Destructive / danger alert background', cssOutput: 'light-dark(#ef4444, #dc2626)' },
  { name: 'success', description: 'Success alert background', cssOutput: 'light-dark(#22c55e, #16a34a)' },
  { name: 'transparent', description: 'Transparent background', cssOutput: 'transparent' },
];

// Text Colors
export const TEXT_COLOR_TOKENS: EdTokenValue[] = [
  { name: 'primary', description: 'Primary high-contrast text color', cssOutput: 'light-dark(#09090b, #fafafa)' },
  { name: 'secondary', description: 'Secondary medium-contrast text color', cssOutput: 'light-dark(#71717a, #a1a1aa)' },
  { name: 'muted', description: 'Muted auxiliary text color', cssOutput: 'light-dark(#a1a1aa, #71717a)' },
  { name: 'dimmed', description: 'Dimmed text color for lower hierarchy', cssOutput: 'light-dark(#a1a1aa, #71717a)' },
  { name: 'on-primary', description: 'Text on primary accent surface (white)', cssOutput: '#ffffff' },
  { name: 'danger', description: 'Destructive / error text color', cssOutput: 'light-dark(#ef4444, #dc2626)' },
  { name: 'red', description: 'Red text color', cssOutput: 'light-dark(#ef4444, #dc2626)' },
  { name: 'success', description: 'Success text color', cssOutput: 'light-dark(#22c55e, #16a34a)' },
  { name: 'green', description: 'Green text color', cssOutput: 'light-dark(#22c55e, #16a34a)' },
  { name: 'blue', description: 'Blue informational text color', cssOutput: 'light-dark(#2563eb, #3b82f6)' },
  { name: 'warning', description: 'Warning text color', cssOutput: 'light-dark(#f59e0b, #fbbf24)' },
  { name: 'yellow', description: 'Yellow text color', cssOutput: 'light-dark(#f59e0b, #fbbf24)' },
  { name: 'inherit', description: 'Inherit text color from parent container', cssOutput: 'inherit' },
];

// Button Palette Colors
export const BUTTON_PALETTE_COLORS: EdTokenValue[] = [
  { name: 'primary', description: 'Default primary accent theme' },
  { name: 'secondary', description: 'Secondary gray theme' },
  { name: 'violet', description: 'Mantine/EDS violet palette' },
  { name: 'indigo', description: 'Indigo palette' },
  { name: 'blue', description: 'Blue palette' },
  { name: 'cyan', description: 'Cyan palette' },
  { name: 'teal', description: 'Teal palette' },
  { name: 'green', description: 'Green palette' },
  { name: 'yellow', description: 'Yellow palette' },
  { name: 'orange', description: 'Orange palette' },
  { name: 'red', description: 'Red palette' },
  { name: 'pink', description: 'Pink palette' },
  { name: 'grape', description: 'Grape palette' },
  { name: 'gray', description: 'Neutral gray palette' },
  { name: 'dark', description: 'Dark charcoal palette' },
  { name: 'danger', description: 'Destructive danger action' },
  { name: 'success', description: 'Success confirmation action' },
  { name: 'warning', description: 'Warning action' },
  { name: 'info', description: 'Informational action' },
];

// Border Tokens
export const BORDER_TOKENS: EdTokenValue[] = [
  { name: 'subtle', description: 'Subtle boundary border (1px)', cssOutput: '1px solid light-dark(#e4e4e7, #27272a)' },
  { name: 'strong', description: 'Prominent high-contrast border', cssOutput: '1px solid light-dark(#d4d4d8, #3f3f46)' },
  { name: 'focus', description: 'Focus ring accent border', cssOutput: '2px solid light-dark(#2563eb, #3b82f6)' },
  { name: 'none', description: 'No border', cssOutput: 'none' },
];

// Radius Tokens
export const RADIUS_TOKENS: EdTokenValue[] = [
  { name: 'none', description: 'Square corners (0px)', cssOutput: '0' },
  { name: 'sm', description: 'Small rounded corners (4px / 0.25rem)', cssOutput: '0.25rem' },
  { name: 'md', description: 'Medium rounded corners (8px / 0.5rem)', cssOutput: '0.5rem' },
  { name: 'lg', description: 'Large rounded corners (12px / 0.75rem)', cssOutput: '0.75rem' },
  { name: 'xl', description: 'Extra large rounded corners (16px / 1rem)', cssOutput: '1rem' },
  { name: 'full', description: 'Fully rounded pill / circle (9999px)', cssOutput: '9999px' },
];

// Shadow Tokens
export const SHADOW_TOKENS: EdTokenValue[] = [
  { name: 'none', description: 'No elevation shadow', cssOutput: 'none' },
  { name: 'sm', description: 'Subtle elevation shadow (cards, buttons)', cssOutput: '0 1px 2px 0 rgba(0, 0, 0, 0.05)' },
  { name: 'md', description: 'Medium elevation shadow (dropdowns, popovers)', cssOutput: '0 4px 6px -1px rgba(0, 0, 0, 0.1)' },
  { name: 'lg', description: 'Large elevation shadow (modals, dialogs)', cssOutput: '0 10px 15px -3px rgba(0, 0, 0, 0.1)' },
];

// Typography Variants
export const TEXT_VARIANTS: EdTokenValue[] = [
  { name: 'display', description: 'Hero display typography (2.25rem / 36px bold)' },
  { name: 'title-lg', description: 'Large page title typography (1.875rem / 30px bold)' },
  { name: 'title-md', description: 'Medium section title typography (1.5rem / 24px bold)' },
  { name: 'heading', description: 'Heading typography (1.25rem / 20px semibold)' },
  { name: 'body', description: 'Default readable body typography (1rem / 16px normal)' },
  { name: 'body-sm', description: 'Small compact body typography (0.875rem / 14px normal)' },
  { name: 'caption', description: 'Auxiliary caption typography (0.75rem / 12px normal)' },
  { name: 'mono', description: 'Monospace code font typography' },
];

// Font Sizes
export const FONT_SIZES: EdTokenValue[] = [
  { name: 'xs', description: 'Extra small font (12px / 0.75rem)' },
  { name: 'sm', description: 'Small font (14px / 0.875rem)' },
  { name: 'md', description: 'Medium base font (16px / 1rem)' },
  { name: 'lg', description: 'Large font (18px / 1.125rem)' },
  { name: 'xl', description: 'Extra large font (20px / 1.25rem)' },
  { name: '2xl', description: '2X large font (24px / 1.5rem)' },
  { name: '3xl', description: '3X large font (30px / 1.875rem)' },
  { name: '4xl', description: '4X large font (36px / 2.25rem)' },
];

// Font Weights
export const FONT_WEIGHTS: EdTokenValue[] = [
  { name: 'light', description: 'Light font weight (300)' },
  { name: 'normal', description: 'Normal body weight (400)' },
  { name: 'medium', description: 'Medium weight (500)' },
  { name: 'semibold', description: 'Semibold weight (600)' },
  { name: 'bold', description: 'Bold weight (700)' },
  { name: 'bolder', description: 'Extra bold weight (800)' },
  { name: 'black', description: 'Black heavy weight (900)' },
];

// Alignments
export const ALIGN_TOKENS: EdTokenValue[] = [
  { name: 'start', description: 'Align to start of cross-axis' },
  { name: 'center', description: 'Align to center of cross-axis' },
  { name: 'end', description: 'Align to end of cross-axis' },
  { name: 'stretch', description: 'Stretch to fill cross-axis' },
  { name: 'baseline', description: 'Align along baseline' },
];

// Text Alignments
export const TEXT_ALIGN_TOKENS: EdTokenValue[] = [
  { name: 'left', description: 'Align text to the left' },
  { name: 'center', description: 'Align text to the center' },
  { name: 'right', description: 'Align text to the right' },
  { name: 'justify', description: 'Justify text with even spacing' },
];

// Justifies
export const JUSTIFY_TOKENS: EdTokenValue[] = [
  { name: 'start', description: 'Pack items toward main-axis start' },
  { name: 'center', description: 'Pack items toward main-axis center' },
  { name: 'end', description: 'Pack items toward main-axis end' },
  { name: 'between', description: 'Distribute items evenly; first at start, last at end' },
  { name: 'around', description: 'Distribute items with equal space around them' },
];

// Max Widths
export const MAX_WIDTH_TOKENS: EdTokenValue[] = [
  { name: 'xs', description: 'Extra small max width (20rem / 320px)' },
  { name: 'sm', description: 'Small max width (24rem / 384px)' },
  { name: 'md', description: 'Medium max width (38rem / 608px)' },
  { name: 'lg', description: 'Large max width (48rem / 768px)' },
  { name: 'xl', description: 'Extra large max width (64rem / 1024px)' },
  { name: '2xl', description: '2X large max width (80rem / 1280px)' },
  { name: '3xl', description: '3X large max width (96rem / 1536px)' },
  { name: 'full', description: 'Full width max (100%)' },
];

// Min Heights
export const MIN_HEIGHT_TOKENS: EdTokenValue[] = [
  { name: 'screen', description: 'Full viewport height (100dvh)' },
  { name: 'full', description: '100% parent container height' },
  { name: 'auto', description: 'Automatic height based on content' },
];

// Button Variants
export const BUTTON_VARIANTS: EdTokenValue[] = [
  { name: 'primary', description: 'High-emphasis solid filled button' },
  { name: 'secondary', description: 'Medium-emphasis subtle/neutral filled button' },
  { name: 'subtle', description: 'Low-emphasis transparent ghost button' },
  { name: 'danger', description: 'Destructive action button' },
  { name: 'default', description: 'Default bordered outline button' },
];

// Button Sizes
export const BUTTON_SIZES: EdTokenValue[] = [
  { name: 'sm', description: 'Small compact button (padding 6px 12px, font-size 13px)' },
  { name: 'md', description: 'Standard medium button (padding 10px 18px, font-size 14px)' },
  { name: 'lg', description: 'Large prominent button (padding 14px 24px, font-size 16px)' },
];

// Badge Statuses
export const BADGE_STATUSES: EdTokenValue[] = [
  { name: 'default', description: 'Neutral gray status badge' },
  { name: 'success', description: 'Green success badge' },
  { name: 'warning', description: 'Yellow warning badge' },
  { name: 'danger', description: 'Red error/danger badge' },
  { name: 'info', description: 'Blue informational badge' },
];

// Title Orders
export const TITLE_ORDERS: EdTokenValue[] = [
  { name: '1', description: 'Heading level 1 (<h1> equivalent, 2.25rem bold)' },
  { name: '2', description: 'Heading level 2 (<h2> equivalent, 1.875rem bold)' },
  { name: '3', description: 'Heading level 3 (<h3> equivalent, 1.5rem bold)' },
  { name: '4', description: 'Heading level 4 (<h4> equivalent, 1.25rem semibold)' },
  { name: '5', description: 'Heading level 5 (<h5> equivalent, 1.125rem semibold)' },
  { name: '6', description: 'Heading level 6 (<h6> equivalent, 1rem semibold)' },
];

// Semantic HTML 'as' Tags
export const AS_ELEMENTS: EdTokenValue[] = [
  { name: 'div', description: 'Generic divider element' },
  { name: 'section', description: 'Thematic grouping of content' },
  { name: 'main', description: 'Dominant content of the document' },
  { name: 'nav', description: 'Navigation links section' },
  { name: 'header', description: 'Introductory or navigational header' },
  { name: 'footer', description: 'Footer for section or page' },
  { name: 'article', description: 'Self-contained composition' },
  { name: 'aside', description: 'Indirectly related content sidebar' },
  { name: 'p', description: 'Paragraph element' },
  { name: 'span', description: 'Inline text element' },
  { name: 'h1', description: 'Top-level heading' },
  { name: 'h2', description: 'Second-level heading' },
  { name: 'h3', description: 'Third-level heading' },
  { name: 'a', description: 'Hyperlink anchor element' },
  { name: 'button', description: 'Interactive button element' },
  { name: 'code', description: 'Inline code snippet' },
  { name: 'pre', description: 'Preformatted text block' },
  { name: 'form', description: 'Interactive form' },
  { name: 'ul', description: 'Unordered list' },
  { name: 'li', description: 'List item' },
];

// Common layout props shared by Box, Stack, Cluster, Card, Center, Group
export const COMMON_LAYOUT_PROPS: Record<string, EdPropInfo> = {
  p: { name: 'p', type: 'SpacingToken', description: 'Padding on all 4 sides (none, xs..3xl)', values: SPACING_TOKENS },
  px: { name: 'px', type: 'SpacingToken | CSS', description: 'Horizontal padding (left and right). Supports tokens (none..3xl) or custom CSS values (24px, 1.5rem)', values: SPACING_TOKENS },
  py: { name: 'py', type: 'SpacingToken | CSS', description: 'Vertical padding (top and bottom). Supports tokens (none..3xl) or custom CSS values (10px, 1rem)', values: SPACING_TOKENS },
  pt: { name: 'pt', type: 'SpacingToken', description: 'Padding top', values: SPACING_TOKENS },
  pb: { name: 'pb', type: 'SpacingToken', description: 'Padding bottom', values: SPACING_TOKENS },
  pl: { name: 'pl', type: 'SpacingToken', description: 'Padding left', values: SPACING_TOKENS },
  pr: { name: 'pr', type: 'SpacingToken', description: 'Padding right', values: SPACING_TOKENS },

  m: { name: 'm', type: 'SpacingToken', description: 'Margin on all 4 sides', values: SPACING_TOKENS },
  mx: { name: 'mx', type: 'SpacingToken', description: 'Horizontal margin (left and right, supports auto)', values: SPACING_TOKENS },
  my: { name: 'my', type: 'SpacingToken', description: 'Vertical margin (top and bottom)', values: SPACING_TOKENS },
  mt: { name: 'mt', type: 'SpacingToken', description: 'Margin top', values: SPACING_TOKENS },
  mb: { name: 'mb', type: 'SpacingToken', description: 'Margin bottom', values: SPACING_TOKENS },
  ml: { name: 'ml', type: 'SpacingToken', description: 'Margin left', values: SPACING_TOKENS },
  mr: { name: 'mr', type: 'SpacingToken', description: 'Margin right', values: SPACING_TOKENS },

  gap: { name: 'gap', type: 'SpacingToken', description: 'Flex gap between child elements', values: SPACING_TOKENS },
  align: { name: 'align', type: 'AlignToken', description: 'Flex align-items cross-axis alignment', values: ALIGN_TOKENS },
  justify: { name: 'justify', type: 'JustifyToken', description: 'Flex justify-content main-axis alignment', values: JUSTIFY_TOKENS },
  direction: { name: 'direction', type: '"row" | "col"', description: 'Flex direction layout', values: [{ name: 'row', description: 'Horizontal flex row' }, { name: 'col', description: 'Vertical flex column' }] },
  wrap: { name: 'wrap', type: '"wrap" | "nowrap" | "wrap-reverse"', description: 'Flex wrap behaviour', values: [{ name: 'wrap', description: 'Allow items to wrap' }, { name: 'nowrap', description: 'No wrap (single line)' }, { name: 'wrap-reverse', description: 'Wrap in reverse' }] },

  bg: { name: 'bg', type: 'SurfaceColorToken', description: 'Surface background color token with native dual-theme support', values: SURFACE_COLOR_TOKENS },
  border: { name: 'border', type: 'BorderToken', description: 'Border style token (subtle, strong, focus, none)', values: BORDER_TOKENS },
  bd: { name: 'bd', type: 'BorderToken', description: 'Alias for border prop', values: BORDER_TOKENS },
  borderTop: { name: 'borderTop', type: 'BorderToken', description: 'Border top style token', values: BORDER_TOKENS },
  borderBottom: { name: 'borderBottom', type: 'BorderToken', description: 'Border bottom style token', values: BORDER_TOKENS },
  borderLeft: { name: 'borderLeft', type: 'BorderToken', description: 'Border left style token', values: BORDER_TOKENS },
  borderRight: { name: 'borderRight', type: 'BorderToken', description: 'Border right style token', values: BORDER_TOKENS },

  radius: { name: 'radius', type: 'RadiusToken', description: 'Corner border-radius token (none, sm, md, lg, xl, full)', values: RADIUS_TOKENS },
  bdrs: { name: 'bdrs', type: 'RadiusToken', description: 'Alias for radius prop', values: RADIUS_TOKENS },
  shadow: { name: 'shadow', type: 'ShadowToken', description: 'Elevation drop shadow token (none, sm, md, lg)', values: SHADOW_TOKENS },

  maw: { name: 'maw', type: 'MaxWidthToken', description: 'Maximum width constraint (xs..3xl, full)', values: MAX_WIDTH_TOKENS },
  maxW: { name: 'maxW', type: 'MaxWidthToken', description: 'Alias for maw', values: MAX_WIDTH_TOKENS },
  mih: { name: 'mih', type: 'MinHeightToken', description: 'Minimum height (screen = 100dvh, full = 100%)', values: MIN_HEIGHT_TOKENS },
  minH: { name: 'minH', type: 'MinHeightToken', description: 'Alias for mih', values: MIN_HEIGHT_TOKENS },
  w: { name: 'w', type: '"full" | "screen" | "auto" | "fit"', description: 'Width sizing', values: [{ name: 'full', description: '100% width' }, { name: 'screen', description: '100vw width' }, { name: 'auto', description: 'Auto width' }, { name: 'fit', description: 'Fit content' }] },
  width: { name: 'width', type: '"full" | "screen" | "auto" | "fit"', description: 'Alias for w prop', values: [{ name: 'full', description: '100% width' }, { name: 'screen', description: '100vw width' }, { name: 'auto', description: 'Auto width' }, { name: 'fit', description: 'Fit content' }] },
  h: { name: 'h', type: '"full" | "screen" | "auto" | "fit"', description: 'Height sizing', values: [{ name: 'full', description: '100% height' }, { name: 'screen', description: '100dvh height' }, { name: 'auto', description: 'Auto height' }, { name: 'fit', description: 'Fit content' }] },
  height: { name: 'height', type: '"full" | "screen" | "auto" | "fit"', description: 'Alias for h prop', values: [{ name: 'full', description: '100% height' }, { name: 'screen', description: '100dvh height' }, { name: 'auto', description: 'Auto height' }, { name: 'fit', description: 'Fit content' }] },

  screen: { name: 'screen', type: 'boolean', description: 'Convenience flag for full viewport screen sizing (width: 100%, min-height: 100dvh)', isBoolean: true },
  center: { name: 'center', type: 'boolean', description: 'Convenience flag to center contents horizontally & vertically', isBoolean: true },
  grow: { name: 'grow', type: 'boolean', description: 'Flex-grow flag (flex-grow: 1)', isBoolean: true },
  flex: { name: 'flex', type: '"1" | "auto" | "none" | "initial"', description: 'CSS flex shorthand property', values: [{ name: '1', description: 'flex: 1 (take remaining space)' }, { name: 'auto', description: 'flex: auto' }, { name: 'none', description: 'flex: none' }] },
  pos: { name: 'pos', type: '"relative" | "absolute" | "fixed" | "sticky"', description: 'CSS position', values: [{ name: 'relative', description: 'Position relative' }, { name: 'absolute', description: 'Position absolute' }, { name: 'fixed', description: 'Position fixed' }, { name: 'sticky', description: 'Position sticky' }] },

  ta: { name: 'ta', type: 'TextAlignToken', description: 'Text alignment (left, center, right, justify)', values: TEXT_ALIGN_TOKENS },
  c: { name: 'c', type: 'TextColorToken', description: 'Text color token (primary, secondary, muted, etc.)', values: TEXT_COLOR_TOKENS },
  color: { name: 'color', type: 'TextColorToken', description: 'Alias for c prop', values: TEXT_COLOR_TOKENS },
  fw: { name: 'fw', type: 'FontWeightToken', description: 'Font weight (light, normal, medium, semibold, bold, black)', values: FONT_WEIGHTS },
  weight: { name: 'weight', type: 'FontWeightToken', description: 'Alias for fw prop', values: FONT_WEIGHTS },
  fz: { name: 'fz', type: 'FontSizeToken', description: 'Font size token (xs..4xl)', values: FONT_SIZES },
  size: { name: 'size', type: 'FontSizeToken', description: 'Alias for fz prop', values: FONT_SIZES },
  tt: { name: 'tt', type: '"uppercase" | "lowercase" | "capitalize" | "none"', description: 'Text transform', values: [{ name: 'uppercase', description: 'UPPERCASE' }, { name: 'lowercase', description: 'lowercase' }, { name: 'capitalize', description: 'Capitalize Words' }, { name: 'none', description: 'none' }] },
  td: { name: 'td', type: '"underline" | "line-through" | "none"', description: 'Text decoration', values: [{ name: 'underline', description: 'Underline text' }, { name: 'line-through', description: 'Strikethrough text' }, { name: 'none', description: 'No decoration' }] },

  as: { name: 'as', type: 'HTMLElement', description: 'Render as semantic HTML element (div, section, main, header, footer, etc.)', values: AS_ELEMENTS },
  class: { name: 'class', type: 'string', description: 'Additional custom CSS classes' },
  id: { name: 'id', type: 'string', description: 'Element unique identifier' },
};

// All EDS Primitives
export const EDS_PRIMITIVES: Record<string, EdPrimitiveInfo> = {
  Box: {
    name: 'Box',
    description: 'Polymorphic layout container supporting flexbox, margins, padding, dual-theme backgrounds, borders, shadows, and semantic `as` tags.',
    defaultTag: 'div',
    snippet: '<Box p="${1:md}">\n\t$0\n</Box>',
    props: { ...COMMON_LAYOUT_PROPS },
  },

  Text: {
    name: 'Text',
    description: 'Typography primitive with intent variants, dual-theme text colors, weights, alignments, and semantic elements.',
    defaultTag: 'p',
    snippet: '<Text variant="${1:body}" c="${2:primary}">$0</Text>',
    props: {
      variant: { name: 'variant', type: 'TextVariantToken', description: 'Typography scale variant (display, title-lg, title-md, heading, body, body-sm, caption, mono)', values: TEXT_VARIANTS },
      c: { name: 'c', type: 'TextColorToken', description: 'Text color intent token (primary, secondary, muted, dimmed, on-primary, danger, success, warning, blue)', values: TEXT_COLOR_TOKENS },
      color: { name: 'color', type: 'TextColorToken', description: 'Alias for c prop', values: TEXT_COLOR_TOKENS },
      fw: { name: 'fw', type: 'FontWeightToken', description: 'Font weight token (light, normal, medium, semibold, bold, black)', values: FONT_WEIGHTS },
      weight: { name: 'weight', type: 'FontWeightToken', description: 'Alias for fw prop', values: FONT_WEIGHTS },
      fz: { name: 'fz', type: 'FontSizeToken', description: 'Font size token (xs..4xl)', values: FONT_SIZES },
      size: { name: 'size', type: 'FontSizeToken', description: 'Alias for fz prop', values: FONT_SIZES },
      ta: { name: 'ta', type: 'TextAlignToken', description: 'Text alignment (left, center, right, justify)', values: TEXT_ALIGN_TOKENS },
      align: { name: 'align', type: 'TextAlignToken', description: 'Alias for ta prop', values: TEXT_ALIGN_TOKENS },
      tt: { name: 'tt', type: '"uppercase" | "lowercase" | "capitalize" | "none"', description: 'Text transform', values: [{ name: 'uppercase', description: 'UPPERCASE' }, { name: 'lowercase', description: 'lowercase' }, { name: 'capitalize', description: 'Capitalize' }, { name: 'none', description: 'none' }] },
      td: { name: 'td', type: '"underline" | "line-through" | "none"', description: 'Text decoration', values: [{ name: 'underline', description: 'Underline' }, { name: 'line-through', description: 'Line-through' }, { name: 'none', description: 'none' }] },
      maw: { name: 'maw', type: 'MaxWidthToken', description: 'Max width constraint', values: MAX_WIDTH_TOKENS },
      maxW: { name: 'maxW', type: 'MaxWidthToken', description: 'Alias for maw', values: MAX_WIDTH_TOKENS },
      truncate: { name: 'truncate', type: 'boolean', description: 'Truncate overflowing text with an ellipsis (...)', isBoolean: true },
      as: { name: 'as', type: 'HTMLElement', description: 'Render as semantic element (p, span, h1..h6, label, code)', values: AS_ELEMENTS },
      m: { name: 'm', type: 'SpacingToken', description: 'Margin on all sides', values: SPACING_TOKENS },
      mx: { name: 'mx', type: 'SpacingToken', description: 'Horizontal margin', values: SPACING_TOKENS },
      my: { name: 'my', type: 'SpacingToken', description: 'Vertical margin', values: SPACING_TOKENS },
      mt: { name: 'mt', type: 'SpacingToken', description: 'Margin top', values: SPACING_TOKENS },
      mb: { name: 'mb', type: 'SpacingToken', description: 'Margin bottom', values: SPACING_TOKENS },
      ml: { name: 'ml', type: 'SpacingToken', description: 'Margin left', values: SPACING_TOKENS },
      mr: { name: 'mr', type: 'SpacingToken', description: 'Margin right', values: SPACING_TOKENS },
      class: { name: 'class', type: 'string', description: 'Custom CSS classes' },
    },
  },

  Stack: {
    name: 'Stack',
    description: 'Vertical flex column layout container with configurable gap, alignment, and full layout styling.',
    defaultTag: 'div',
    snippet: '<Stack gap="${1:md}">\n\t$0\n</Stack>',
    props: { ...COMMON_LAYOUT_PROPS },
  },

  Cluster: {
    name: 'Cluster',
    description: 'Horizontal flex row layout container with automatic wrapping, gap spacing, and alignment.',
    defaultTag: 'div',
    snippet: '<Cluster gap="${1:md}" align="${2:center}">\n\t$0\n</Cluster>',
    props: { ...COMMON_LAYOUT_PROPS },
  },

  Group: {
    name: 'Group',
    description: 'Horizontal flex row container specifically optimized for grouping actions, buttons, and toolbar elements.',
    defaultTag: 'div',
    snippet: '<Group gap="${1:md}" align="center">\n\t$0\n</Group>',
    props: { ...COMMON_LAYOUT_PROPS },
  },

  Center: {
    name: 'Center',
    description: 'Flex container that centers its children horizontally and vertically (`justify-content: center; align-items: center`).',
    defaultTag: 'div',
    snippet: '<Center screen p="${1:xl}" bg="${2:canvas}">\n\t$0\n</Center>',
    props: { ...COMMON_LAYOUT_PROPS },
  },

  Card: {
    name: 'Card',
    description: 'Elevated surface container with built-in dual-theme card background, subtle border, rounded corners, and shadow.',
    defaultTag: 'div',
    snippet: '<Card p="${1:2xl}" maw="${2:md}">\n\t$0\n</Card>',
    props: { ...COMMON_LAYOUT_PROPS },
  },

  Button: {
    name: 'Button',
    description: 'Interactive button component supporting intent variants, sizes, palette colors, custom CSS colors, link mode (`as="a"`), and event handlers.',
    defaultTag: 'button',
    snippet: '<Button variant="${1|primary,secondary,subtle,danger|}" size="${2|md,sm,lg|}">\n\t$0\n</Button>',
    props: {
      variant: { name: 'variant', type: '"primary" | "secondary" | "subtle" | "danger" | "default"', description: 'Visual intent style variant', values: BUTTON_VARIANTS },
      size: { name: 'size', type: '"sm" | "md" | "lg"', description: 'Button size dimensions', values: BUTTON_SIZES },
      color: { name: 'color', type: 'PaletteColor | CSS', description: 'Mantine/EDS palette color name (violet, indigo, blue, green, red, etc.) or custom CSS color (#2B5B69)', values: BUTTON_PALETTE_COLORS },
      c: { name: 'c', type: 'TextColorToken', description: 'Text color token or custom hex/CSS color', values: TEXT_COLOR_TOKENS },
      as: { name: 'as', type: '"button" | "a"', description: 'Render as button or hyperlink anchor tag', values: [{ name: 'button', description: 'Standard clickable button' }, { name: 'a', description: 'Hyperlink anchor element' }] },
      href: { name: 'href', type: 'string', description: 'Hyperlink URL (used when as="a")' },
      target: { name: 'target', type: 'string', description: 'Link target window (_blank, _self)', values: [{ name: '_blank', description: 'Open in new tab' }, { name: '_self', description: 'Open in current tab' }] },
      rel: { name: 'rel', type: 'string', description: 'Link relation attribute (e.g. noopener noreferrer)' },
      disabled: { name: 'disabled', type: 'boolean', description: 'Disable button interactions', isBoolean: true },
      type: { name: 'type', type: '"button" | "submit" | "reset"', description: 'Button form type', values: [{ name: 'button', description: 'Regular button' }, { name: 'submit', description: 'Submit form' }, { name: 'reset', description: 'Reset form' }] },
      onClick: { name: 'onClick', type: 'function', description: 'Click event handler callback function' },
      px: { name: 'px', type: 'SpacingToken | CSS', description: 'Horizontal button padding (e.g. 24px, 1.5rem)', values: SPACING_TOKENS },
      py: { name: 'py', type: 'SpacingToken | CSS', description: 'Vertical button padding (e.g. 10px, 0.75rem)', values: SPACING_TOKENS },
      p: { name: 'p', type: 'SpacingToken', description: 'Uniform padding', values: SPACING_TOKENS },
      m: { name: 'm', type: 'SpacingToken', description: 'Margin around button', values: SPACING_TOKENS },
      mt: { name: 'mt', type: 'SpacingToken', description: 'Margin top', values: SPACING_TOKENS },
      mb: { name: 'mb', type: 'SpacingToken', description: 'Margin bottom', values: SPACING_TOKENS },
      ml: { name: 'ml', type: 'SpacingToken', description: 'Margin left', values: SPACING_TOKENS },
      mr: { name: 'mr', type: 'SpacingToken', description: 'Margin right', values: SPACING_TOKENS },
      fw: { name: 'fw', type: 'FontWeightToken', description: 'Font weight', values: FONT_WEIGHTS },
      tt: { name: 'tt', type: '"uppercase" | "lowercase" | "capitalize" | "none"', description: 'Text transform', values: [{ name: 'uppercase', description: 'UPPERCASE' }, { name: 'none', description: 'none' }] },
      td: { name: 'td', type: '"underline" | "line-through" | "none"', description: 'Text decoration', values: [{ name: 'none', description: 'No decoration' }, { name: 'underline', description: 'Underline' }] },
      class: { name: 'class', type: 'string', description: 'Custom CSS classes' },
    },
  },

  Badge: {
    name: 'Badge',
    description: 'Compact status chip indicator supporting semantic status tokens (success, warning, danger, info, default).',
    defaultTag: 'span',
    snippet: '<Badge status="${1|info,success,warning,danger,default|}">$0</Badge>',
    props: {
      status: { name: 'status', type: '"default" | "success" | "warning" | "danger" | "info"', description: 'Semantic status intent color', values: BADGE_STATUSES },
      tt: { name: 'tt', type: '"uppercase" | "lowercase" | "capitalize" | "none"', description: 'Text transform (default uppercase in EDS)', values: [{ name: 'uppercase', description: 'UPPERCASE' }, { name: 'none', description: 'none' }] },
      px: { name: 'px', type: 'SpacingToken | CSS', description: 'Horizontal badge padding', values: SPACING_TOKENS },
      py: { name: 'py', type: 'SpacingToken | CSS', description: 'Vertical badge padding', values: SPACING_TOKENS },
      p: { name: 'p', type: 'SpacingToken', description: 'Padding on badge', values: SPACING_TOKENS },
      m: { name: 'm', type: 'SpacingToken', description: 'Margin around badge', values: SPACING_TOKENS },
      class: { name: 'class', type: 'string', description: 'Custom CSS classes' },
    },
  },

  Title: {
    name: 'Title',
    description: 'Semantic heading component mapped to h1-h6 with configurable typography scale, weights, colors, and alignments.',
    defaultTag: 'h1',
    snippet: '<Title order="${1|1,2,3,4,5,6|}" fz="${2:3xl}" fw="${3:bold}" c="${4:primary}">$0</Title>',
    props: {
      order: { name: 'order', type: '"1" | "2" | "3" | "4" | "5" | "6"', description: 'Heading rank (1 generates h1, 2 generates h2, etc.)', values: TITLE_ORDERS },
      c: { name: 'c', type: 'TextColorToken', description: 'Text color intent token', values: TEXT_COLOR_TOKENS },
      color: { name: 'color', type: 'TextColorToken', description: 'Alias for c prop', values: TEXT_COLOR_TOKENS },
      fw: { name: 'fw', type: 'FontWeightToken', description: 'Font weight', values: FONT_WEIGHTS },
      weight: { name: 'weight', type: 'FontWeightToken', description: 'Alias for fw prop', values: FONT_WEIGHTS },
      fz: { name: 'fz', type: 'FontSizeToken', description: 'Font size token (xs..4xl)', values: FONT_SIZES },
      size: { name: 'size', type: 'FontSizeToken', description: 'Alias for fz prop', values: FONT_SIZES },
      ta: { name: 'ta', type: 'TextAlignToken', description: 'Text alignment (left, center, right, justify)', values: TEXT_ALIGN_TOKENS },
      align: { name: 'align', type: 'TextAlignToken', description: 'Alias for ta prop', values: TEXT_ALIGN_TOKENS },
      tt: { name: 'tt', type: '"uppercase" | "lowercase" | "capitalize" | "none"', description: 'Text transform', values: [{ name: 'uppercase', description: 'UPPERCASE' }, { name: 'none', description: 'none' }] },
      td: { name: 'td', type: '"underline" | "line-through" | "none"', description: 'Text decoration', values: [{ name: 'underline', description: 'Underline' }, { name: 'none', description: 'none' }] },
      as: { name: 'as', type: 'HTMLElement', description: 'Override semantic tag (h1..h6, p, div)', values: AS_ELEMENTS },
      m: { name: 'm', type: 'SpacingToken', description: 'Margin around heading', values: SPACING_TOKENS },
      mt: { name: 'mt', type: 'SpacingToken', description: 'Margin top', values: SPACING_TOKENS },
      mb: { name: 'mb', type: 'SpacingToken', description: 'Margin bottom', values: SPACING_TOKENS },
      class: { name: 'class', type: 'string', description: 'Custom CSS classes' },
    },
  },

  Code: {
    name: 'Code',
    description: 'Monospace code primitive for inline snippets or preformatted code blocks with dual-theme background.',
    defaultTag: 'code',
    snippet: '<Code>$0</Code>',
    props: {
      block: { name: 'block', type: 'boolean', description: 'Render as block-level code container (pre)', isBoolean: true },
      c: { name: 'c', type: 'TextColorToken', description: 'Text color token', values: TEXT_COLOR_TOKENS },
      color: { name: 'color', type: 'TextColorToken', description: 'Alias for c prop', values: TEXT_COLOR_TOKENS },
      bg: { name: 'bg', type: 'SurfaceColorToken', description: 'Background color token (card, muted, etc.)', values: SURFACE_COLOR_TOKENS },
      fz: { name: 'fz', type: 'FontSizeToken', description: 'Font size token', values: FONT_SIZES },
      px: { name: 'px', type: 'SpacingToken | CSS', description: 'Horizontal padding', values: SPACING_TOKENS },
      py: { name: 'py', type: 'SpacingToken | CSS', description: 'Vertical padding', values: SPACING_TOKENS },
      p: { name: 'p', type: 'SpacingToken', description: 'Padding', values: SPACING_TOKENS },
      m: { name: 'm', type: 'SpacingToken', description: 'Margin', values: SPACING_TOKENS },
      class: { name: 'class', type: 'string', description: 'Custom CSS classes' },
    },
  },
};
