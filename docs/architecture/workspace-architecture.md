# Velowork Workspace & State Architecture (`velowork-workspace`)

The `velowork-workspace` crate coordinates workspace layout trees, focus management, project hierarchies, persistence serialization, and workspace session settings.

---

## 1. Layered Crate Architecture

```text
velowork-workspace (Coordinator GPUI Entity, Persistence, Actions)
  ├── velowork-layout (Recursive LayoutNode Tree, Split & Normalize Algorithms)
  └── velowork-state (Pure Domain Data: WorkspaceData, ProjectData, FolderData, ServiceDefinition)
```

| Crate | Responsibility |
| :--- | :--- |
| `velowork-state` | Pure data representations (`WorkspaceData`, `ProjectData`, `FolderData`, `ServiceDefinition`, `Toast`). Completely decoupled from GPUI. |
| `velowork-layout` | Recursive `LayoutNode` split tree structure, geometric calculation, and normalization algorithms. |
| `velowork-workspace` | The `Workspace` GPUI entity, action dispatchers, settings coordinator, and disk persistence. |

---

## 2. Key Types & Components

- **`Workspace`** (`state.rs`): Root GPUI observable entity wrapping `WorkspaceData`. Tracks active project selections, layout trees, remote sync states, and access histories.
- **`FocusManager`** (`focus.rs`): Bounded stack for hierarchical focus navigation and tab restore.
- **`RequestBroker`** (`request_broker.rs`): Decoupled asynchronous event broker allowing background subsystems to request UI actions without creating circular entity dependencies.
- **`SettingsState`** (`settings.rs`): Reactive preferences container loading from `settings.json` with debounced auto-save.

---

## 3. Layout Model & Persistence

### Recursive `LayoutNode` Tree

Terminal panels are organized as a recursive tree of horizontal and vertical splits:
- Navigated via immutable index paths (`Vec<usize>`).
- Layout mutations (split, close, swap, reorder, resize) produce normalized tree geometry.

### Serialization & Backward Compatibility

- Persistence is saved to `workspace.json` (and `velowork.db` SQLite store) with debounced write operations.
- State fields utilize `#[serde(default)]` to guarantee backward and forward schema compatibility across version upgrades.
