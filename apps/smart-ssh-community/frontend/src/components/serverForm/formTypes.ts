import type { useConnectionTest } from "./useConnectionTest";
import type { useIdentityFile } from "./useIdentityFile";
import type { useServerDelete } from "./useServerDelete";
import type { useServerFormModel } from "./useServerFormModel";

export type ServerTab = "general" | "connection" | "security" | "notes";

export type ServerModel = ReturnType<typeof useServerFormModel>;
export type ConnectionTestState = ReturnType<typeof useConnectionTest>;
export type ServerDeleteState = ReturnType<typeof useServerDelete>;
export type IdentityFileState = ReturnType<typeof useIdentityFile>;
