import { loadWorkTasks } from "./pages/work.js";
import { render } from "./shell/router.js";
import { applyNavCollapsed } from "./state.js";
import "./shell/dialogs.js";
import "./shell/actions.js";
import "./shell/sidebar.js";

applyNavCollapsed();
if (!location.hash) {
  try { history.replaceState(null, "", "#/home"); } catch (e) {}
}
loadWorkTasks();
render(true);
