use super::*;

#[test]
fn test_ermcss_utility_compilation() {
    let content = r#"
    <div class="flex items-center justify-center min-h-screen bg-gray-100">
        <h2 class="text-2xl font-bold text-gray-800">Hello Eronom</h2>
    </div>
    "#;
    let params = std::collections::HashMap::new();
    
    let classes = vec![
        "flex".to_string(),
        "items-center".to_string(),
        "justify-center".to_string(),
        "min-h-screen".to_string(),
        "bg-gray-100".to_string(),
        "text-2xl".to_string(),
        "font-bold".to_string(),
        "text-gray-800".to_string(),
    ];
    if let Some(compiler_path) = find_ermcss_path(".") {
        let css = run_ermcss_compiler(&compiler_path, std::path::Path::new("."), &classes).unwrap();
        set_global_ermcss(css);
    }
    
    let res = process_erm_component(".", content, false, &params).unwrap();
    println!("ERMCSS COMPILATION RES:\n{}", res);
    
    assert!(res.contains(".flex { display: flex; }"));
    assert!(res.contains(".items-center { align-items: center; }"));
    assert!(res.contains(".justify-center { justify-content: center; }"));
    assert!(res.contains(".min-h-screen { min-height: 100vh; }"));
    assert!(res.contains(".bg-gray-100 { background-color: #f3f4f6; }"));
    assert!(res.contains(".text-2xl { font-size: 1.5rem; line-height: 2rem; }"));
    assert!(res.contains(".font-bold { font-weight: 700; }"));
    assert!(res.contains(".text-gray-800 { color: #1f2937; }"));
}

#[test]
fn test_testp_ermcss_compilation() {
    let base_path = std::path::Path::new("testp");
    let cfg = parse_ermcss_config(base_path);
    println!("CFG ENABLED: {}, GLOBS: {:?}", cfg.enabled, cfg.content);
    let res = compile_project_ermcss(base_path, &cfg.content).unwrap();
    println!("COMPILED CSS LENGTH: {}", res.len());
    println!("COMPILED CSS:\n{}", res);
}

#[test]
fn test_function_based_template() {
    let content = r#"
    import Header from './Header.erm';

    export fn page(params) {
        let name = useState('world');
        <h1>Hello {name} from {params.id}</h1>
    }
    <style>
        h1 { color: red; }
    </style>
    "#;
    
    let preprocessed = preprocess_function_template(content).unwrap();
    println!("PREPROCESSED:\n{}", preprocessed);
    assert!(preprocessed.contains("<script>"));
    assert!(preprocessed.contains("let params = useParams();"));
    assert!(preprocessed.contains("let name = useState('world');"));
    assert!(preprocessed.contains("<h1 data-erm-line=\"6\">Hello {name} from {params.id}</h1>"));
    assert!(preprocessed.contains("<style>"));
}

#[test]
fn test_link_compilation() {
    let content = "<Link to=\"/contact\">Contact</Link>";
    let mut visited = std::collections::HashMap::new();
    let mut if_counter = 0;
    let mut for_counter = 0;
    let params = std::collections::HashMap::new();
    let mut state_var_sources = std::collections::HashMap::new();
    let res = process_component_tree(".", content, &mut visited, None, &params, &mut if_counter, &mut for_counter, &mut state_var_sources).unwrap();
    assert!(res.html.contains("<a"));
    assert!(res.html.contains("href=\"/contact\""));
    assert!(res.html.contains(">Contact</a>"));
}

#[test]
fn test_use_state_compilation() {
    let content = r#"
    <script>
        let count = useState(0);
    </script>
    <button onClick={()=>{count++}}>Count {count}</button>
    "#;
    let mut visited = std::collections::HashMap::new();
    let mut if_counter = 0;
    let mut for_counter = 0;
    let params = std::collections::HashMap::new();
    let mut state_var_sources = std::collections::HashMap::new();
    let res = process_component_tree(".", content, &mut visited, None, &params, &mut if_counter, &mut for_counter, &mut state_var_sources).unwrap();
    assert!(res.state_vars.contains(&"count".to_string()));
    let combined = res.scripts.join("\n");
    assert!(combined.contains("useState(0, \"___count\")"));
    assert!(combined.contains("count.value++"));
}

#[test]
fn test_fragment_compilation() {
    let content = r#"
    export fn page(params) {
        let count = useState(0);
        <>
            <h1>Hello from function based template!</h1>
            <p>Current count is: {count}</p>
            <button onClick={() => { count++ }}>Increment</button>
        </>
    }
    "#;
    let mut visited = std::collections::HashMap::new();
    let mut if_counter = 0;
    let mut for_counter = 0;
    let params = std::collections::HashMap::new();
    let mut state_var_sources = std::collections::HashMap::new();
    let res = process_component_tree(".", content, &mut visited, None, &params, &mut if_counter, &mut for_counter, &mut state_var_sources).unwrap();
    assert!(!res.html.contains("<>"));
    assert!(!res.html.contains("</>"));
    assert!(res.html.contains("Hello from function based template!</h1>"));
    assert!(res.html.contains("Increment</button>"));
}

#[test]
fn test_for_loop_compilation() {
    let content = r#"
    <script>
        let items = useState([1, 2, 3]);
    </script>
    for item, i in items {
        <p>Item key as {i} : {item}</p>
    }
    "#;
    let params = std::collections::HashMap::new();
    let res = process_erm_component(".", content, true, &params).unwrap();
    println!("{}", res);
    assert!(res.contains("Item key as 0 : 1"));
    assert!(res.contains("() => (items.value)"));
    assert!(!res.contains("items.value.value"));
}

#[test]
fn test_contact_page_id() {
    let content = r#"
    <script>
        let isSlowDataLoaded = useState(false);
        let slowMessage = useState('');
    </script>
    <Loading fallback={<div>Loading...</div>}>
        <p>Status: {isSlowDataLoaded ? ('✓ ' + slowMessage) : '⚡ Syncing API Data...'}</p>
    </Loading>
    "#;
    let mut visited = std::collections::HashMap::new();
    let mut if_counter = 0;
    let mut for_counter = 0;
    let params = std::collections::HashMap::new();
    let mut state_var_sources = std::collections::HashMap::new();
    let tree_res = process_component_tree("libs/init/app/pages/contact.erm", &content, &mut visited, None, &params, &mut if_counter, &mut for_counter, &mut state_var_sources).unwrap();
    assert!(!tree_res.html.is_empty());
    let params = std::collections::HashMap::new();
    let res = process_erm_component("libs/init/app/pages/contact.erm", &content, true, &params).unwrap();
    println!("=== CONTACT COMPILED RESULT ===\n{}\n===============================", res);
    assert!(!res.is_empty());
    assert!(!res.contains("Status: false"));
    assert!(res.contains("Status:"));
    assert!(res.contains("⚡ Syncing API Data..."));
}

#[test]
fn test_index_page_compilation() {
    let content = std::fs::read_to_string("libs/init/app/pages/index.erm").unwrap();
    let mut visited = std::collections::HashMap::new();
    let mut if_counter = 0;
    let mut for_counter = 0;
    let params = std::collections::HashMap::new();
    let mut state_var_sources = std::collections::HashMap::new();
    let tree_res = process_component_tree("libs/init/app/pages/index.erm", &content, &mut visited, None, &params, &mut if_counter, &mut for_counter, &mut state_var_sources).unwrap();
    assert!(!tree_res.html.is_empty());
    let res = process_erm_component("libs/init/app/pages/index.erm", &content, true, &params).unwrap();
    assert!(!res.is_empty());
    assert!(res.contains("Get started by editing"));
    assert!(res.contains("app/pages/index.erm"));
    assert!(res.contains("Documentation"));
}

#[test]
fn test_new_if_syntax_compiler() {
    let content = r#"
    <script>
        let porridge = { temperature: 90 };
    </script>
    if porridge.temperature > 100 {
        <p>too hot!</p>
    } else if 80 > porridge.temperature {
        <p>too cold!</p>
    } else {
        <p>just right!</p>
    }
    "#;
    let params = std::collections::HashMap::new();
    let res = process_erm_component(".", content, false, &params).unwrap();
    println!("RES HTML:\n{}", res);
    let html_part = res.split("<script type=\"module\" class=\"__erm_script\">").next().unwrap();
    assert!(html_part.contains("just right!"));
    assert!(!html_part.contains("too hot!"));
    assert!(!html_part.contains("too cold!"));
    assert!(res.contains("erm-if-0"));
}

#[test]
fn test_nested_if_syntax_compiler() {
    let content = r#"
    <script>
        let outer = true;
        let inner = false;
    </script>
    if outer {
        if inner {
            <p>Inner True</p>
        } else {
            <p>Inner False</p>
        }
    }
    "#;
    let params = std::collections::HashMap::new();
    let res = process_erm_component(".", content, false, &params).unwrap();
    println!("NESTED RES:\n{}", res);
    let html_part = res.split("<script type=\"module\" class=\"__erm_script\">").next().unwrap();
    assert!(html_part.contains("Inner False"));
    assert!(!html_part.contains("Inner True"));
    assert!(res.contains("erm-if-0"));
    assert!(res.contains("erm-if-1"));
}

#[test]
fn test_transform_use_effect_fn() {
    assert_eq!(
        transform_use_effect("useEffect(() => { console.log(count); }, [count])"),
        "useEffect(() => { console.log(count); }, () => [count])"
    );
    assert_eq!(
        transform_use_effect("useEffect(() => {}, [])"),
        "useEffect(() => {}, () => [])"
    );
    assert_eq!(
        transform_use_effect("useEffect(() => { const x = [1, 2]; }, [count, x])"),
        "useEffect(() => { const x = [1, 2]; }, () => [count, x])"
    );
}

#[test]
fn test_loading_tag_compilation() {
    let content = r#"
    <script>
        let ready = useState(false);
    </script>
    <Loading fallback={<div>Loading skeleton...</div>}>
        <div>Actual Content Loaded</div>
    </Loading>
    "#;
    let params = std::collections::HashMap::new();
    let res = process_erm_component(".", content, false, &params).unwrap();
    assert!(res.contains("erm-suspense-container"));
    assert!(res.contains("Loading skeleton..."));
    assert!(res.contains("Actual Content Loaded"));
}

#[test]
fn test_fine_grained_reactivity_compilation() {
    let content = r#"
    <script>
        let count = useState(0);
    </script>
    <button onClick={() => { count++ }}>Count {count}</button>
    "#;
    let mut visited = std::collections::HashMap::new();
    let mut if_counter = 0;
    let mut for_counter = 0;
    let params = std::collections::HashMap::new();
    let mut state_var_sources = std::collections::HashMap::new();
    let res = process_component_tree(".", content, &mut visited, None, &params, &mut if_counter, &mut for_counter, &mut state_var_sources).unwrap();
    let combined = res.scripts.join("\n");
    assert!(combined.contains("bindText("));
    assert!(combined.contains("bindEvent("));
    assert!(!combined.contains("window.__erm_bindings"));
    assert!(!combined.contains("window.__erm_events"));
    assert!(!combined.contains("window.__erm_update"));
}

#[test]
fn test_createroot_module_generation() {
    let content = r#"
    <script>
        let count = useState(0);
    </script>
    <h1>Count: {count}</h1>
    "#;
    let params = std::collections::HashMap::new();
    let res = process_erm_component(".", content, false, &params).unwrap();
    assert!(res.contains("createRoot((dispose) => {"));
    assert!(res.contains("import {"));
    assert!(res.contains("createSignal"));
    assert!(res.contains("createEffect"));
    assert!(res.contains("bindText"));
    assert!(!res.contains("window.__erm_init_reactivity"));
    assert!(!res.contains("window.__erm_update"));
}

#[test]
fn test_clean_blocks_compilation() {
    let content = r#"
    <script>
        let items = useState([1, 2]);
        let show = useState(true);
    </script>
    if show {
        <p>Visible</p>
    }
    for item in items {
        <span>Item: {item}</span>
    }
    "#;
    let params = std::collections::HashMap::new();
    let res = process_erm_component(".", content, false, &params).unwrap();
    assert!(res.contains("renderIf("));
    assert!(res.contains("renderFor("));
    assert!(!res.contains("window.__erm_register_if"));
    assert!(!res.contains("window.__erm_register_for"));
}

#[test]
fn test_eds_valid_primitive_compilation() {
    let content = r#"
    <Box as="nav" direction="row" gap="md" p="md" bg="card" border="subtle">
        <Text as="h1" variant="title-lg" weight="bold" color="primary">Eronom Showcase</Text>
        <Stack gap="sm">
            <Card>
                <Text variant="body" color="secondary">Card Description</Text>
                <Cluster gap="xs" align="center">
                    <Badge status="success">Active</Badge>
                    <Button variant="primary" size="sm">Save</Button>
                </Cluster>
            </Card>
        </Stack>
    </Box>
    "#;
    let params = std::collections::HashMap::new();
    let res = process_erm_component(".", content, false, &params).unwrap();
    println!("EDS COMPILED RES:\n{}", res);

    assert!(res.contains("<nav"));
    assert!(res.contains("class=\"eds-box eds-row eds-gap-md eds-p-md eds-bg-card eds-border-subtle\""));
    assert!(res.contains("<h1"));
    assert!(res.contains("class=\"eds-text eds-variant-title-lg eds-color-primary eds-weight-bold\""));
    assert!(res.contains("class=\"eds-card\""));
    assert!(res.contains("class=\"eds-badge eds-badge-success\""));
    assert!(res.contains("class=\"eds-btn eds-btn-primary eds-btn-sm\""));
    assert!(res.contains("</nav>"));
    assert!(res.contains("</h1>"));

    // Check light-dark() CSS variables
    assert!(res.contains("color-scheme: light dark;"));
    assert!(res.contains("--eds-bg-card: light-dark("));
    assert!(res.contains("--eds-text-primary: light-dark("));
}

#[test]
fn test_eds_invalid_token_rejection() {
    let content = r#"
    <Box p="17px">
        <Text>Hello</Text>
    </Box>
    "#;
    let params = std::collections::HashMap::new();
    let err = process_erm_component(".", content, false, &params);
    assert!(err.is_err());
    let err_msg = err.err().unwrap().to_string();
    assert!(err_msg.contains("Invalid spacing token '17px' on <Box>"));

    let content_bad_bg = r#"
    <Box bg="neon-pink">
        <Text>Hello</Text>
    </Box>
    "#;
    let err_bg = process_erm_component(".", content_bad_bg, false, &params);
    assert!(err_bg.is_err());
    let err_bg_msg = err_bg.err().unwrap().to_string();
    assert!(err_bg_msg.contains("Invalid background color token 'neon-pink' on <Box>"));

    let content_bad_variant = r#"
    <Text variant="gigantic">Hello</Text>
    "#;
    let err_var = process_erm_component(".", content_bad_variant, false, &params);
    assert!(err_var.is_err());
    let err_var_msg = err_var.err().unwrap().to_string();
    assert!(err_var_msg.contains("Invalid typography variant 'gigantic' on <Text>"));
}

#[test]
fn test_eds_bare_div_allowed() {
    let config = DesignSystemConfig::default();
    let mut attrs = std::collections::HashMap::new();
    attrs.insert("class".to_string(), "\"p-4\"".to_string());
    let res = config.validate_tag("div", &attrs, "test.erm", 10);
    assert!(res.is_ok());
}

#[test]
fn test_eds_demo_page_compilation() {
    let demo_path = std::path::Path::new("testp/app/pages/eds-demo.erm");
    if demo_path.exists() {
        let content = std::fs::read_to_string(demo_path).unwrap();
        let params = std::collections::HashMap::new();
        let res = process_erm_component("testp/app/pages/eds-demo.erm", &content, false, &params).unwrap();
        println!("COMPILED EDS DEMO RES:\n{}", res);
        assert!(res.contains("class=\"eds-box"));
        assert!(res.contains("class=\"eds-card\""));
        assert!(res.contains("class=\"eds-btn"));
        assert!(res.contains("class=\"eds-badge"));
        assert!(res.contains("color-scheme: light dark;"));
    }
}



