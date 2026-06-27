import { describe, expect, it, vi } from "vitest";

// Simple unit test to verify test infrastructure works
describe("App test infrastructure", () => {
  it("test setup loads without errors", () => {
    // This test verifies that the test infrastructure (localStorage polyfill,
    // jest-dom, vitest config) is working correctly.
    expect(true).toBe(true);
  });

  it("vi mock functions work", () => {
    const mockFn = vi.fn(() => "test");
    expect(mockFn()).toBe("test");
    expect(mockFn).toHaveBeenCalledTimes(1);
  });

  it("async operations can be mocked", async () => {
    const mockPromise = Promise.resolve("success");
    await expect(mockPromise).resolves.toBe("success");
  });
});
