import { describe, expect, it, vi } from "vitest";

// Simple unit test to verify test infrastructure works for dialogs
describe("Dialogs test infrastructure", () => {
  it("test setup loads without errors", () => {
    // This test verifies that the test infrastructure is working correctly.
    expect(true).toBe(true);
  });

  it("mock callbacks work", () => {
    const mockCallback = vi.fn();
    mockCallback();
    expect(mockCallback).toHaveBeenCalledTimes(1);
  });

  it("can test simple component props", () => {
    type DialogProps = { text: string; onClose: () => void };
    
    const props: DialogProps = {
      text: "Test message",
      onClose: vi.fn(),
    };
    
    expect(props.text).toBe("Test message");
    expect(typeof props.onClose).toBe("function");
  });
});
