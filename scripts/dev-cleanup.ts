import { cleanupAllDevProcesses } from "./dev-lifecycle";

cleanupAllDevProcesses(["5173", "5174", "8765", "8766"]);
console.log("Stopped PatchOpsIII dev processes on ports 5173, 5174, 8765, and 8766.");
