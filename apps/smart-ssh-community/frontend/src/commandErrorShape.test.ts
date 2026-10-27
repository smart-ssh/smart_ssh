import { describe, expect, it } from "vitest";
import fixture from "../../../../crates/core/tests/fixtures/command_error_shapes.json";
import {
  commandErrorConnectLog,
  commandErrorFeatureLocked,
  commandErrorCode,
  commandErrorMessage,
} from "./api";

// The same file pins the Rust serialisation of `ssh_manager_core::ipc_error::CommandError`.
describe("command error JSON shapes", () => {
  it("reads a plain message", () => {
    const e = fixture.message_only;
    expect(commandErrorMessage(e)).toBe("Something failed");
    expect(commandErrorCode(e)).toBeNull();
    expect(commandErrorFeatureLocked(e)).toBeNull();
    expect(commandErrorConnectLog(e)).toBeNull();
  });

  it("reads message and code", () => {
    const e = fixture.with_code;
    expect(commandErrorMessage(e)).toBe("Group cannot be its own parent");
    expect(commandErrorCode(e)).toBe("GROUP_SELF_PARENT");
    expect(commandErrorFeatureLocked(e)).toBeNull();
  });

  it("reads feature_locked", () => {
    const e = fixture.feature_locked;
    expect(commandErrorCode(e)).toBe("FEATURE_LOCKED");
    expect(commandErrorFeatureLocked(e)).toEqual({ feature: "document_export", tier: "free" });
  });

  it("reads connect_log", () => {
    const e = fixture.connect_log;
    expect(commandErrorMessage(e)).toBe("Connection failed");
    expect(commandErrorCode(e)).toBe("SSH_CONNECT_FAILED");
    const log = commandErrorConnectLog(e);
    expect(log).toHaveLength(1);
    expect(log?.[0].hop).toBe("alice@example.org:22");
  });
});
