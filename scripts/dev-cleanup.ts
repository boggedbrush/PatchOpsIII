import { cleanupAllDevProcesses } from "./dev-lifecycle";

cleanupAllDevProcesses(["5173", "5174", "5175", "8765", "8766", "8767"]);
console.log("Stopped PatchOpsIII dev processes on ports 5173, 5174, 5175, 8765, 8766, and 8767.");
