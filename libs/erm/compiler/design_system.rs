use std::collections::{HashMap, HashSet};
use anyhow::{bail, Result};

#[derive(Clone, Debug)]
pub struct ColorToken {
    pub light: String,
    pub dark: String,
}

#[derive(Clone, Debug)]
pub struct DesignSystemConfig {
    pub enabled: bool,
    pub colors: HashMap<String, ColorToken>,
    pub text_colors: HashMap<String, ColorToken>,
    pub borders: HashMap<String, ColorToken>,
    pub spacings: HashMap<String, String>,
    pub radii: HashMap<String, String>,
    pub shadows: HashMap<String, String>,
    pub allowed_spacings: HashSet<String>,
    pub allowed_colors: HashSet<String>,
    pub allowed_text_colors: HashSet<String>,
    pub allowed_borders: HashSet<String>,
    pub allowed_radii: HashSet<String>,
    pub allowed_shadows: HashSet<String>,
    pub allowed_text_variants: HashSet<String>,
    pub allowed_weights: HashSet<String>,
    pub allowed_aligns: HashSet<String>,
    pub allowed_justifies: HashSet<String>,
    pub allowed_directions: HashSet<String>,
    pub allowed_as_elements: HashSet<String>,
}

impl Default for DesignSystemConfig {
    fn default() -> Self {
        let mut cfg = Self {
            enabled: true,
            colors: HashMap::new(),
            text_colors: HashMap::new(),
            borders: HashMap::new(),
            spacings: HashMap::new(),
            radii: HashMap::new(),
            shadows: HashMap::new(),
            allowed_spacings: HashSet::new(),
            allowed_colors: HashSet::new(),
            allowed_text_colors: HashSet::new(),
            allowed_borders: HashSet::new(),
            allowed_radii: HashSet::new(),
            allowed_shadows: HashSet::new(),
            allowed_text_variants: HashSet::new(),
            allowed_weights: HashSet::new(),
            allowed_aligns: HashSet::new(),
            allowed_justifies: HashSet::new(),
            allowed_directions: HashSet::new(),
            allowed_as_elements: HashSet::new(),
        };

        cfg.init_defaults();
        cfg
    }
}

impl DesignSystemConfig {
    pub fn init_defaults(&mut self) {
        // Surface & Background colors
        self.colors.insert("canvas".to_string(), ColorToken {
            light: "hsl(0, 0%, 100%)".to_string(),
            dark: "hsl(240, 10%, 4%)".to_string(),
        });
        self.colors.insert("card".to_string(), ColorToken {
            light: "hsl(0, 0%, 98%)".to_string(),
            dark: "hsl(240, 8%, 8%)".to_string(),
        });
        self.colors.insert("muted".to_string(), ColorToken {
            light: "hsl(240, 5%, 96%)".to_string(),
            dark: "hsl(240, 6%, 12%)".to_string(),
        });
        self.colors.insert("overlay".to_string(), ColorToken {
            light: "hsl(0, 0%, 100%)".to_string(),
            dark: "hsl(240, 8%, 10%)".to_string(),
        });
        self.colors.insert("primary".to_string(), ColorToken {
            light: "hsl(221, 83%, 53%)".to_string(),
            dark: "hsl(217, 91%, 60%)".to_string(),
        });
        self.colors.insert("danger".to_string(), ColorToken {
            light: "hsl(0, 84%, 60%)".to_string(),
            dark: "hsl(0, 72%, 51%)".to_string(),
        });
        self.colors.insert("success".to_string(), ColorToken {
            light: "hsl(142, 71%, 45%)".to_string(),
            dark: "hsl(142, 70%, 40%)".to_string(),
        });
        self.colors.insert("transparent".to_string(), ColorToken {
            light: "transparent".to_string(),
            dark: "transparent".to_string(),
        });

        // Typography colors
        self.text_colors.insert("primary".to_string(), ColorToken {
            light: "hsl(240, 10%, 4%)".to_string(),
            dark: "hsl(0, 0%, 98%)".to_string(),
        });
        self.text_colors.insert("secondary".to_string(), ColorToken {
            light: "hsl(240, 5%, 45%)".to_string(),
            dark: "hsl(240, 5%, 65%)".to_string(),
        });
        self.text_colors.insert("muted".to_string(), ColorToken {
            light: "hsl(240, 4%, 60%)".to_string(),
            dark: "hsl(240, 4%, 45%)".to_string(),
        });
        self.text_colors.insert("on-primary".to_string(), ColorToken {
            light: "hsl(0, 0%, 100%)".to_string(),
            dark: "hsl(0, 0%, 100%)".to_string(),
        });
        self.text_colors.insert("danger".to_string(), ColorToken {
            light: "hsl(0, 84%, 60%)".to_string(),
            dark: "hsl(0, 72%, 51%)".to_string(),
        });
        self.text_colors.insert("success".to_string(), ColorToken {
            light: "hsl(142, 71%, 45%)".to_string(),
            dark: "hsl(142, 70%, 40%)".to_string(),
        });

        // Borders
        self.borders.insert("subtle".to_string(), ColorToken {
            light: "hsl(240, 6%, 90%)".to_string(),
            dark: "hsl(240, 6%, 16%)".to_string(),
        });
        self.borders.insert("strong".to_string(), ColorToken {
            light: "hsl(240, 6%, 80%)".to_string(),
            dark: "hsl(240, 6%, 24%)".to_string(),
        });
        self.borders.insert("focus".to_string(), ColorToken {
            light: "hsl(221, 83%, 53%)".to_string(),
            dark: "hsl(217, 91%, 60%)".to_string(),
        });
        self.borders.insert("none".to_string(), ColorToken {
            light: "transparent".to_string(),
            dark: "transparent".to_string(),
        });

        // Spacings
        self.spacings.insert("none".to_string(), "0".to_string());
        self.spacings.insert("xs".to_string(), "0.25rem".to_string());
        self.spacings.insert("sm".to_string(), "0.5rem".to_string());
        self.spacings.insert("md".to_string(), "0.75rem".to_string());
        self.spacings.insert("lg".to_string(), "1rem".to_string());
        self.spacings.insert("xl".to_string(), "1.5rem".to_string());
        self.spacings.insert("2xl".to_string(), "2rem".to_string());
        self.spacings.insert("3xl".to_string(), "3rem".to_string());

        // Radii
        self.radii.insert("none".to_string(), "0".to_string());
        self.radii.insert("sm".to_string(), "0.25rem".to_string());
        self.radii.insert("md".to_string(), "0.5rem".to_string());
        self.radii.insert("lg".to_string(), "0.75rem".to_string());
        self.radii.insert("xl".to_string(), "1rem".to_string());
        self.radii.insert("full".to_string(), "9999px".to_string());

        // Shadows
        self.shadows.insert("none".to_string(), "none".to_string());
        self.shadows.insert("sm".to_string(), "0 1px 2px 0 rgba(0, 0, 0, 0.05)".to_string());
        self.shadows.insert("md".to_string(), "0 4px 6px -1px rgba(0, 0, 0, 0.1)".to_string());
        self.shadows.insert("lg".to_string(), "0 10px 15px -3px rgba(0, 0, 0, 0.1)".to_string());

        // Fill allowed sets
        self.allowed_colors = self.colors.keys().cloned().collect();
        self.allowed_text_colors = self.text_colors.keys().cloned().collect();
        self.allowed_borders = self.borders.keys().cloned().collect();
        self.allowed_spacings = self.spacings.keys().cloned().collect();
        self.allowed_radii = self.radii.keys().cloned().collect();
        self.allowed_shadows = self.shadows.keys().cloned().collect();

        self.allowed_text_variants = [
            "display", "title-lg", "title-md", "heading", "body", "body-sm", "caption", "mono",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();

        self.allowed_weights = ["normal", "medium", "semibold", "bold"]
            .iter()
            .map(|s| s.to_string())
            .collect();

        self.allowed_aligns = ["start", "center", "end", "stretch", "baseline"]
            .iter()
            .map(|s| s.to_string())
            .collect();

        self.allowed_justifies = ["start", "center", "end", "between", "around"]
            .iter()
            .map(|s| s.to_string())
            .collect();

        self.allowed_directions = ["row", "col"].iter().map(|s| s.to_string()).collect();

        self.allowed_as_elements = [
            "div", "section", "nav", "main", "header", "footer", "article", "aside", "ul", "li",
            "p", "span", "h1", "h2", "h3", "h4", "h5", "h6", "label", "code", "a", "button", "form",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
    }

    /// Generate complete CSS with CSS light-dark() root variables and atomic EDS classes
    pub fn generate_root_css(&self) -> String {
        let mut out = String::new();
        out.push_str("/* === Eronom Design System (EDS): LLM-Safe Tokens & Styles === */\n");
        out.push_str(":root {\n  color-scheme: light dark;\n");

        // Surface / Background variables
        for (k, v) in &self.colors {
            out.push_str(&format!(
                "  --eds-bg-{}: light-dark({}, {});\n",
                k, v.light, v.dark
            ));
        }

        // Text variables
        for (k, v) in &self.text_colors {
            out.push_str(&format!(
                "  --eds-text-{}: light-dark({}, {});\n",
                k, v.light, v.dark
            ));
        }

        // Border variables
        for (k, v) in &self.borders {
            out.push_str(&format!(
                "  --eds-border-{}: light-dark({}, {});\n",
                k, v.light, v.dark
            ));
        }

        // Spacing variables
        for (k, v) in &self.spacings {
            out.push_str(&format!("  --eds-space-{}: {};\n", k, v));
        }

        // Radii variables
        for (k, v) in &self.radii {
            out.push_str(&format!("  --eds-radius-{}: {};\n", k, v));
        }

        // Shadow variables
        for (k, v) in &self.shadows {
            out.push_str(&format!("  --eds-shadow-{}: {};\n", k, v));
        }

        out.push_str("}\n\n");

        // Box layout helpers
        out.push_str(".eds-box { box-sizing: border-box; }\n");
        out.push_str(".eds-row { display: flex; flex-direction: row; }\n");
        out.push_str(".eds-col { display: flex; flex-direction: column; }\n");
        out.push_str(".eds-wrap { flex-wrap: wrap; }\n");

        // Align & Justify
        out.push_str(".eds-align-start { align-items: flex-start; }\n");
        out.push_str(".eds-align-center { align-items: center; }\n");
        out.push_str(".eds-align-end { align-items: flex-end; }\n");
        out.push_str(".eds-align-stretch { align-items: stretch; }\n");
        out.push_str(".eds-align-baseline { align-items: baseline; }\n");

        out.push_str(".eds-justify-start { justify-content: flex-start; }\n");
        out.push_str(".eds-justify-center { justify-content: center; }\n");
        out.push_str(".eds-justify-end { justify-content: flex-end; }\n");
        out.push_str(".eds-justify-between { justify-content: space-between; }\n");
        out.push_str(".eds-justify-around { justify-content: space-around; }\n");

        // Widths & Heights
        out.push_str(".eds-w-full { width: 100%; }\n");
        out.push_str(".eds-w-screen { width: 100vw; }\n");
        out.push_str(".eds-w-auto { width: auto; }\n");
        out.push_str(".eds-w-fit { width: fit-content; }\n");

        out.push_str(".eds-h-full { height: 100%; }\n");
        out.push_str(".eds-h-screen { height: 100vh; }\n");
        out.push_str(".eds-h-auto { height: auto; }\n");

        // Gaps
        for k in self.spacings.keys() {
            out.push_str(&format!(".eds-gap-{} {{ gap: var(--eds-space-{}); }}\n", k, k));
        }

        // Paddings
        for k in self.spacings.keys() {
            out.push_str(&format!(".eds-p-{} {{ padding: var(--eds-space-{}); }}\n", k, k));
            out.push_str(&format!(
                ".eds-px-{} {{ padding-left: var(--eds-space-{}); padding-right: var(--eds-space-{}); }}\n",
                k, k, k
            ));
            out.push_str(&format!(
                ".eds-py-{} {{ padding-top: var(--eds-space-{}); padding-bottom: var(--eds-space-{}); }}\n",
                k, k, k
            ));
            out.push_str(&format!(".eds-pt-{} {{ padding-top: var(--eds-space-{}); }}\n", k, k));
            out.push_str(&format!(".eds-pb-{} {{ padding-bottom: var(--eds-space-{}); }}\n", k, k));
            out.push_str(&format!(".eds-pl-{} {{ padding-left: var(--eds-space-{}); }}\n", k, k));
            out.push_str(&format!(".eds-pr-{} {{ padding-right: var(--eds-space-{}); }}\n", k, k));
        }

        // Backgrounds
        for k in self.colors.keys() {
            out.push_str(&format!(".eds-bg-{} {{ background-color: var(--eds-bg-{}); }}\n", k, k));
        }

        // Borders
        for k in self.borders.keys() {
            out.push_str(&format!(".eds-border-{} {{ border: 1px solid var(--eds-border-{}); }}\n", k, k));
            out.push_str(&format!(".eds-border-t-{} {{ border-top: 1px solid var(--eds-border-{}); }}\n", k, k));
            out.push_str(&format!(".eds-border-b-{} {{ border-bottom: 1px solid var(--eds-border-{}); }}\n", k, k));
            out.push_str(&format!(".eds-border-l-{} {{ border-left: 1px solid var(--eds-border-{}); }}\n", k, k));
            out.push_str(&format!(".eds-border-r-{} {{ border-right: 1px solid var(--eds-border-{}); }}\n", k, k));
        }

        // Radii
        for k in self.radii.keys() {
            out.push_str(&format!(".eds-radius-{} {{ border-radius: var(--eds-radius-{}); }}\n", k, k));
        }

        // Shadows
        for k in self.shadows.keys() {
            out.push_str(&format!(".eds-shadow-{} {{ box-shadow: var(--eds-shadow-{}); }}\n", k, k));
        }

        // Typography
        out.push_str(".eds-text { margin: 0; font-family: inherit; }\n");
        out.push_str(".eds-variant-display { font-size: 2.25rem; line-height: 2.5rem; font-weight: 700; letter-spacing: -0.025em; }\n");
        out.push_str(".eds-variant-title-lg { font-size: 1.875rem; line-height: 2.25rem; font-weight: 600; letter-spacing: -0.02em; }\n");
        out.push_str(".eds-variant-title-md { font-size: 1.5rem; line-height: 2rem; font-weight: 600; letter-spacing: -0.015em; }\n");
        out.push_str(".eds-variant-heading { font-size: 1.25rem; line-height: 1.75rem; font-weight: 600; }\n");
        out.push_str(".eds-variant-body { font-size: 1rem; line-height: 1.5rem; font-weight: 400; }\n");
        out.push_str(".eds-variant-body-sm { font-size: 0.875rem; line-height: 1.25rem; font-weight: 400; }\n");
        out.push_str(".eds-variant-caption { font-size: 0.75rem; line-height: 1rem; font-weight: 500; }\n");
        out.push_str(".eds-variant-mono { font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace; font-size: 0.875rem; }\n");

        for k in self.text_colors.keys() {
            out.push_str(&format!(".eds-color-{} {{ color: var(--eds-text-{}); }}\n", k, k));
        }

        out.push_str(".eds-weight-normal { font-weight: 400; }\n");
        out.push_str(".eds-weight-medium { font-weight: 500; }\n");
        out.push_str(".eds-weight-semibold { font-weight: 600; }\n");
        out.push_str(".eds-weight-bold { font-weight: 700; }\n");

        out.push_str(".eds-text-left { text-align: left; }\n");
        out.push_str(".eds-text-center { text-align: center; }\n");
        out.push_str(".eds-text-right { text-align: right; }\n");
        out.push_str(".eds-truncate { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }\n");

        // High-level UI Components
        out.push_str(".eds-card { display: flex; flex-direction: column; background-color: var(--eds-bg-card); border: 1px solid var(--eds-border-subtle); border-radius: var(--eds-radius-lg); box-shadow: var(--eds-shadow-sm); padding: var(--eds-space-lg); box-sizing: border-box; }\n");
        out.push_str(".eds-btn { display: inline-flex; align-items: center; justify-content: center; gap: var(--eds-space-xs); font-family: inherit; font-weight: 600; border-radius: var(--eds-radius-md); border: 1px solid transparent; cursor: pointer; transition: background-color 0.15s ease, opacity 0.15s ease; box-sizing: border-box; text-decoration: none; }\n");
        out.push_str(".eds-btn-primary { background-color: var(--eds-bg-primary); color: var(--eds-text-on-primary); }\n");
        out.push_str(".eds-btn-primary:hover { opacity: 0.9; }\n");
        out.push_str(".eds-btn-secondary { background-color: var(--eds-bg-muted); color: var(--eds-text-primary); border-color: var(--eds-border-subtle); }\n");
        out.push_str(".eds-btn-secondary:hover { background-color: var(--eds-bg-card); }\n");
        out.push_str(".eds-btn-subtle { background-color: transparent; color: var(--eds-text-secondary); }\n");
        out.push_str(".eds-btn-subtle:hover { background-color: var(--eds-bg-muted); color: var(--eds-text-primary); }\n");
        out.push_str(".eds-btn-danger { background-color: var(--eds-bg-danger); color: var(--eds-text-on-primary); }\n");
        out.push_str(".eds-btn-danger:hover { opacity: 0.9; }\n");
        out.push_str(".eds-btn-sm { padding: 0.25rem 0.5rem; font-size: 0.75rem; }\n");
        out.push_str(".eds-btn-md { padding: 0.5rem 1rem; font-size: 0.875rem; }\n");
        out.push_str(".eds-btn-lg { padding: 0.75rem 1.25rem; font-size: 1rem; }\n");

        out.push_str(".eds-badge { display: inline-flex; align-items: center; font-size: 0.75rem; font-weight: 600; padding: 0.125rem 0.5rem; border-radius: var(--eds-radius-full); box-sizing: border-box; }\n");
        out.push_str(".eds-badge-default { background-color: var(--eds-bg-muted); color: var(--eds-text-secondary); }\n");
        out.push_str(".eds-badge-success { background-color: rgba(34, 197, 94, 0.15); color: var(--eds-text-success); }\n");
        out.push_str(".eds-badge-warning { background-color: rgba(234, 179, 8, 0.15); color: #d97706; }\n");
        out.push_str(".eds-badge-danger { background-color: rgba(239, 68, 68, 0.15); color: var(--eds-text-danger); }\n");
        out.push_str(".eds-badge-info { background-color: rgba(59, 130, 246, 0.15); color: var(--eds-bg-primary); }\n");

        out
    }

    /// Validates an AST element tag and its attributes against the EDS rules.
    pub fn validate_tag(&self, tag_name: &str, attrs: &HashMap<String, String>, file: &str, line: usize) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }

        // Token verification for primitives
        match tag_name {
            "Box" | "Stack" | "Cluster" => {
                for (key, val) in attrs {
                    let val_trimmed = val.trim_matches('"').trim_matches('\'').trim();
                    match key.as_str() {
                        "p" | "px" | "py" | "pt" | "pb" | "pl" | "pr" | "gap" => {
                            if !self.allowed_spacings.contains(val_trimmed) {
                                bail!(
                                    "[EDS Error] {}:{}: Invalid spacing token '{}' on <{}>.\nAllowed values: {:?}",
                                    file, line, val_trimmed, tag_name, self.allowed_spacings
                                );
                            }
                        }
                        "bg" => {
                            if !self.allowed_colors.contains(val_trimmed) {
                                bail!(
                                    "[EDS Error] {}:{}: Invalid background color token '{}' on <{}>.\nAllowed values: {:?}",
                                    file, line, val_trimmed, tag_name, self.allowed_colors
                                );
                            }
                        }
                        "border" | "borderTop" | "borderBottom" | "borderLeft" | "borderRight" => {
                            if !self.allowed_borders.contains(val_trimmed) {
                                bail!(
                                    "[EDS Error] {}:{}: Invalid border token '{}' on <{}>.\nAllowed values: {:?}",
                                    file, line, val_trimmed, tag_name, self.allowed_borders
                                );
                            }
                        }
                        "radius" => {
                            if !self.allowed_radii.contains(val_trimmed) {
                                bail!(
                                    "[EDS Error] {}:{}: Invalid radius token '{}' on <{}>.\nAllowed values: {:?}",
                                    file, line, val_trimmed, tag_name, self.allowed_radii
                                );
                            }
                        }
                        "shadow" => {
                            if !self.allowed_shadows.contains(val_trimmed) {
                                bail!(
                                    "[EDS Error] {}:{}: Invalid shadow token '{}' on <{}>.\nAllowed values: {:?}",
                                    file, line, val_trimmed, tag_name, self.allowed_shadows
                                );
                            }
                        }
                        "align" => {
                            if !self.allowed_aligns.contains(val_trimmed) {
                                bail!(
                                    "[EDS Error] {}:{}: Invalid align token '{}' on <{}>.\nAllowed values: {:?}",
                                    file, line, val_trimmed, tag_name, self.allowed_aligns
                                );
                            }
                        }
                        "justify" => {
                            if !self.allowed_justifies.contains(val_trimmed) {
                                bail!(
                                    "[EDS Error] {}:{}: Invalid justify token '{}' on <{}>.\nAllowed values: {:?}",
                                    file, line, val_trimmed, tag_name, self.allowed_justifies
                                );
                            }
                        }
                        "direction" => {
                            if !self.allowed_directions.contains(val_trimmed) {
                                bail!(
                                    "[EDS Error] {}:{}: Invalid direction token '{}' on <{}>.\nAllowed values: {:?}",
                                    file, line, val_trimmed, tag_name, self.allowed_directions
                                );
                            }
                        }
                        "as" => {
                            if !self.allowed_as_elements.contains(val_trimmed) {
                                bail!(
                                    "[EDS Error] {}:{}: Invalid 'as' element '{}' on <{}>.\nAllowed values: {:?}",
                                    file, line, val_trimmed, tag_name, self.allowed_as_elements
                                );
                            }
                        }
                        _ => {}
                    }
                }
            }
            "Text" => {
                for (key, val) in attrs {
                    let val_trimmed = val.trim_matches('"').trim_matches('\'').trim();
                    match key.as_str() {
                        "variant" => {
                            if !self.allowed_text_variants.contains(val_trimmed) {
                                bail!(
                                    "[EDS Error] {}:{}: Invalid typography variant '{}' on <Text>.\nAllowed values: {:?}",
                                    file, line, val_trimmed, self.allowed_text_variants
                                );
                            }
                        }
                        "color" => {
                            if !self.allowed_text_colors.contains(val_trimmed) {
                                bail!(
                                    "[EDS Error] {}:{}: Invalid text color token '{}' on <Text>.\nAllowed values: {:?}",
                                    file, line, val_trimmed, self.allowed_text_colors
                                );
                            }
                        }
                        "weight" => {
                            if !self.allowed_weights.contains(val_trimmed) {
                                bail!(
                                    "[EDS Error] {}:{}: Invalid font weight token '{}' on <Text>.\nAllowed values: {:?}",
                                    file, line, val_trimmed, self.allowed_weights
                                );
                            }
                        }
                        "as" => {
                            if !self.allowed_as_elements.contains(val_trimmed) {
                                bail!(
                                    "[EDS Error] {}:{}: Invalid 'as' element '{}' on <Text>.\nAllowed values: {:?}",
                                    file, line, val_trimmed, self.allowed_as_elements
                                );
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }

        Ok(())
    }

    /// Transforms an EDS primitive into a native semantic HTML element with atomic EDS classes.
    pub fn transform_primitive(
        &self,
        tag_name: &str,
        attrs: &HashMap<String, String>,
    ) -> Option<(String, String)> {
        match tag_name {
            "Box" => {
                let html_tag = attrs
                    .get("as")
                    .map(|s| s.trim_matches('"').trim_matches('\'').to_string())
                    .unwrap_or_else(|| "div".to_string());

                let mut classes = vec!["eds-box".to_string()];

                if let Some(dir) = attrs.get("direction") {
                    let dir_val = dir.trim_matches('"').trim_matches('\'');
                    if dir_val == "row" {
                        classes.push("eds-row".to_string());
                    } else if dir_val == "col" {
                        classes.push("eds-col".to_string());
                    }
                }

                if let Some(gap) = attrs.get("gap") {
                    let gap_val = gap.trim_matches('"').trim_matches('\'');
                    classes.push(format!("eds-gap-{}", gap_val));
                }

                if let Some(align) = attrs.get("align") {
                    let align_val = align.trim_matches('"').trim_matches('\'');
                    classes.push(format!("eds-align-{}", align_val));
                }

                if let Some(justify) = attrs.get("justify") {
                    let justify_val = justify.trim_matches('"').trim_matches('\'');
                    classes.push(format!("eds-justify-{}", justify_val));
                }

                if let Some(p) = attrs.get("p") {
                    classes.push(format!("eds-p-{}", p.trim_matches('"').trim_matches('\'')));
                }
                if let Some(px) = attrs.get("px") {
                    classes.push(format!("eds-px-{}", px.trim_matches('"').trim_matches('\'')));
                }
                if let Some(py) = attrs.get("py") {
                    classes.push(format!("eds-py-{}", py.trim_matches('"').trim_matches('\'')));
                }
                if let Some(pt) = attrs.get("pt") {
                    classes.push(format!("eds-pt-{}", pt.trim_matches('"').trim_matches('\'')));
                }
                if let Some(pb) = attrs.get("pb") {
                    classes.push(format!("eds-pb-{}", pb.trim_matches('"').trim_matches('\'')));
                }
                if let Some(pl) = attrs.get("pl") {
                    classes.push(format!("eds-pl-{}", pl.trim_matches('"').trim_matches('\'')));
                }
                if let Some(pr) = attrs.get("pr") {
                    classes.push(format!("eds-pr-{}", pr.trim_matches('"').trim_matches('\'')));
                }

                if let Some(bg) = attrs.get("bg") {
                    classes.push(format!("eds-bg-{}", bg.trim_matches('"').trim_matches('\'')));
                }

                if let Some(border) = attrs.get("border") {
                    classes.push(format!("eds-border-{}", border.trim_matches('"').trim_matches('\'')));
                }
                if let Some(b) = attrs.get("borderTop") {
                    classes.push(format!("eds-border-t-{}", b.trim_matches('"').trim_matches('\'')));
                }
                if let Some(b) = attrs.get("borderBottom") {
                    classes.push(format!("eds-border-b-{}", b.trim_matches('"').trim_matches('\'')));
                }
                if let Some(b) = attrs.get("borderLeft") {
                    classes.push(format!("eds-border-l-{}", b.trim_matches('"').trim_matches('\'')));
                }
                if let Some(b) = attrs.get("borderRight") {
                    classes.push(format!("eds-border-r-{}", b.trim_matches('"').trim_matches('\'')));
                }

                if let Some(r) = attrs.get("radius") {
                    classes.push(format!("eds-radius-{}", r.trim_matches('"').trim_matches('\'')));
                }

                if let Some(s) = attrs.get("shadow") {
                    classes.push(format!("eds-shadow-{}", s.trim_matches('"').trim_matches('\'')));
                }

                if let Some(w) = attrs.get("width") {
                    classes.push(format!("eds-w-{}", w.trim_matches('"').trim_matches('\'')));
                }
                if let Some(h) = attrs.get("height") {
                    classes.push(format!("eds-h-{}", h.trim_matches('"').trim_matches('\'')));
                }

                // Forward passthrough attributes (class, id, style, onClick, etc.)
                let mut passthrough = String::new();
                for (k, v) in attrs {
                    match k.as_str() {
                        "as" | "direction" | "gap" | "align" | "justify" | "p" | "px" | "py"
                        | "pt" | "pb" | "pl" | "pr" | "bg" | "border" | "borderTop"
                        | "borderBottom" | "borderLeft" | "borderRight" | "radius" | "shadow"
                        | "width" | "height" => {}
                        "class" => {
                            let user_class = v.trim_matches('"').trim_matches('\'');
                            if !user_class.is_empty() {
                                classes.push(user_class.to_string());
                            }
                        }
                        _ => {
                            passthrough.push_str(&format_passthrough_attr(k, v));
                        }
                    }
                }

                let open_tag = format!("<{} class=\"{}\"{}>", html_tag, classes.join(" "), passthrough);
                Some((open_tag, html_tag))
            }
            "Text" => {
                let html_tag = attrs
                    .get("as")
                    .map(|s| s.trim_matches('"').trim_matches('\'').to_string())
                    .unwrap_or_else(|| "p".to_string());

                let mut classes = vec!["eds-text".to_string()];

                let variant = attrs
                    .get("variant")
                    .map(|s| s.trim_matches('"').trim_matches('\''))
                    .unwrap_or("body");
                classes.push(format!("eds-variant-{}", variant));

                if let Some(c) = attrs.get("color") {
                    classes.push(format!("eds-color-{}", c.trim_matches('"').trim_matches('\'')));
                }
                if let Some(w) = attrs.get("weight") {
                    classes.push(format!("eds-weight-{}", w.trim_matches('"').trim_matches('\'')));
                }
                if let Some(a) = attrs.get("align") {
                    classes.push(format!("eds-text-{}", a.trim_matches('"').trim_matches('\'')));
                }
                if attrs.contains_key("truncate") {
                    classes.push("eds-truncate".to_string());
                }

                let mut passthrough = String::new();
                for (k, v) in attrs {
                    match k.as_str() {
                        "as" | "variant" | "color" | "weight" | "align" | "truncate" => {}
                        "class" => {
                            let user_class = v.trim_matches('"').trim_matches('\'');
                            if !user_class.is_empty() {
                                classes.push(user_class.to_string());
                            }
                        }
                        _ => {
                            passthrough.push_str(&format_passthrough_attr(k, v));
                        }
                    }
                }

                let open_tag = format!("<{} class=\"{}\"{}>", html_tag, classes.join(" "), passthrough);
                Some((open_tag, html_tag))
            }
            "Stack" => {
                let mut box_attrs = attrs.clone();
                box_attrs.insert("direction".to_string(), "\"col\"".to_string());
                self.transform_primitive("Box", &box_attrs)
            }
            "Cluster" => {
                let mut box_attrs = attrs.clone();
                box_attrs.insert("direction".to_string(), "\"row\"".to_string());
                if let Some(res) = self.transform_primitive("Box", &box_attrs) {
                    let open_with_wrap = res.0.replacen("class=\"eds-box eds-row", "class=\"eds-box eds-row eds-wrap", 1);
                    Some((open_with_wrap, res.1))
                } else {
                    None
                }
            }
            "Card" => {
                let html_tag = attrs
                    .get("as")
                    .map(|s| s.trim_matches('"').trim_matches('\'').to_string())
                    .unwrap_or_else(|| "div".to_string());

                let mut classes = vec!["eds-card".to_string()];
                let mut passthrough = String::new();

                for (k, v) in attrs {
                    match k.as_str() {
                        "as" => {}
                        "class" => {
                            let user_class = v.trim_matches('"').trim_matches('\'');
                            if !user_class.is_empty() {
                                classes.push(user_class.to_string());
                            }
                        }
                        _ => {
                            passthrough.push_str(&format_passthrough_attr(k, v));
                        }
                    }
                }

                let open_tag = format!("<{} class=\"{}\"{}>", html_tag, classes.join(" "), passthrough);
                Some((open_tag, html_tag))
            }
            "Button" => {
                let html_tag = attrs
                    .get("as")
                    .map(|s| s.trim_matches('"').trim_matches('\'').to_string())
                    .unwrap_or_else(|| "button".to_string());

                let variant = attrs
                    .get("variant")
                    .map(|s| s.trim_matches('"').trim_matches('\''))
                    .unwrap_or("primary");
                let size = attrs
                    .get("size")
                    .map(|s| s.trim_matches('"').trim_matches('\''))
                    .unwrap_or("md");

                let mut classes = vec![
                    "eds-btn".to_string(),
                    format!("eds-btn-{}", variant),
                    format!("eds-btn-{}", size),
                ];

                let mut passthrough = String::new();
                for (k, v) in attrs {
                    match k.as_str() {
                        "as" | "variant" | "size" => {}
                        "class" => {
                            let user_class = v.trim_matches('"').trim_matches('\'');
                            if !user_class.is_empty() {
                                classes.push(user_class.to_string());
                            }
                        }
                        _ => {
                            passthrough.push_str(&format_passthrough_attr(k, v));
                        }
                    }
                }

                let open_tag = format!("<{} class=\"{}\"{}>", html_tag, classes.join(" "), passthrough);
                Some((open_tag, html_tag))
            }
            "Badge" => {
                let status = attrs
                    .get("status")
                    .map(|s| s.trim_matches('"').trim_matches('\''))
                    .unwrap_or("default");

                let mut classes = vec![
                    "eds-badge".to_string(),
                    format!("eds-badge-{}", status),
                ];

                let mut passthrough = String::new();
                for (k, v) in attrs {
                    match k.as_str() {
                        "status" => {}
                        "class" => {
                            let user_class = v.trim_matches('"').trim_matches('\'');
                            if !user_class.is_empty() {
                                classes.push(user_class.to_string());
                            }
                        }
                        _ => {
                            passthrough.push_str(&format_passthrough_attr(k, v));
                        }
                    }
                }

                let open_tag = format!("<span class=\"{}\"{}>", classes.join(" "), passthrough);
                Some((open_tag, "span".to_string()))
            }
            _ => None,
        }
    }
}

fn format_passthrough_attr(k: &str, v: &str) -> String {
    let v_trimmed = v.trim();
    if v_trimmed.starts_with('{') && v_trimmed.ends_with('}') {
        format!(" {}={}", k, v_trimmed)
    } else if v_trimmed.is_empty() {
        format!(" {}", k)
    } else {
        format!(" {}=\"{}\"", k, v_trimmed.trim_matches('"').trim_matches('\''))
    }
}
