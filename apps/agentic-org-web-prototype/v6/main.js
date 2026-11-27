import { loadWorkTasks } from "./pages/work.js";
import { render } from "./shell/router.js";
import { applyNavCollapsed } from "./state.js";
import "./shell/dialogs.js";
import "./shell/actions.js";
import "./shell/sidebar.js";
import "./pages/person-panel.js";
import "./pages/project-plan-actions.js";
import "./pages/processes-actions.js";
import "./pages/docs-actions.js";
import { mountDevProcesses } from "./pages/dev-processes.js";

applyNavCollapsed();
if (!location.hash) {
  try { history.replaceState(null, "", "#/home"); } catch (e) {}
}
loadWorkTasks();
render(true);
mountDevProcesses();
