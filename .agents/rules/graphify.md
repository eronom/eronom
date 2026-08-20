---
trigger: always_on
---

# Graphify Knowledge Graph Integration Guidelines

This project utilizes a graphify knowledge graph stored at `graphify-out/`.

## Navigation & Query Rules
1. **Architecture Discovery**: Read `graphify-out/GRAPH_REPORT.md` to understand central nodes and community structure before answering complex architectural questions.
2. **Wiki Traversal**: If `graphify-out/wiki/index.md` exists, navigate it instead of inspecting raw source files directly.
3. **MCP Tools**: If the graphify MCP server is active, use tools like `query_graph`, `get_node`, and `shortest_path` for navigation.
4. **CLI Tools**: If the MCP server is inactive, use `graphify query "<question>"`, `graphify path "<A>" "<B>"`, and `graphify explain "<concept>"` to trace cross-module relationships.
5. **Graph Maintenance**: After modifying source files in a session, run `graphify update .` to update the AST graph.
