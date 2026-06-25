import { render, screen } from "@solidjs/testing-library";
import { lazy } from "solid-js";
import { describe, expect, it } from "vitest";
import { LazyViewBoundary } from "./lazyViews";

describe("LazyViewBoundary", () => {
  it("replaces the fallback when a lazy advanced view resolves", async () => {
    const AdvancedView = lazy(async () => ({
      default: () => <section>Advanced view loaded</section>,
    }));

    render(() => (
      <LazyViewBoundary>
        <AdvancedView />
      </LazyViewBoundary>
    ));

    expect(screen.getByRole("status")).toHaveTextContent("Loading view");
    expect(await screen.findByText("Advanced view loaded")).toBeInTheDocument();
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
  });
});
