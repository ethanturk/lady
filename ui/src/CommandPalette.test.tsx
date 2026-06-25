import { fireEvent, render, screen } from "@solidjs/testing-library";
import { describe, expect, it, vi } from "vitest";
import CommandPalette, { type PaletteEntry } from "./CommandPalette";

describe("CommandPalette", () => {
  const entries = (run = vi.fn()): PaletteEntry[] => [
    { kind: "action", label: "Open repository", run },
    { kind: "branch", label: "feature/settings", run: vi.fn() },
    { kind: "file", label: "src-tauri/src/main.rs", run: vi.fn() },
  ];

  it("filters entries and runs the selected action", () => {
    const run = vi.fn();
    const onClose = vi.fn();

    render(() => <CommandPalette open entries={entries(run)} onOpen={vi.fn()} onClose={onClose} />);

    const input = screen.getByRole("combobox", { name: /jump to action/i });
    fireEvent.input(input, { target: { value: "open" } });

    expect(screen.getByRole("option", { name: /open repository/i })).toBeInTheDocument();
    expect(screen.queryByText("feature/settings")).not.toBeInTheDocument();

    fireEvent.keyDown(input, { key: "Enter" });

    expect(run).toHaveBeenCalledOnce();
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("closes on escape without running an entry", () => {
    const run = vi.fn();
    const onClose = vi.fn();

    render(() => <CommandPalette open entries={entries(run)} onOpen={vi.fn()} onClose={onClose} />);

    fireEvent.keyDown(screen.getByRole("combobox"), { key: "Escape" });

    expect(run).not.toHaveBeenCalled();
    expect(onClose).toHaveBeenCalledOnce();
  });
});
