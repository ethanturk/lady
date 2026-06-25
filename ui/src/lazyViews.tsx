import { lazy, Suspense } from "solid-js";
import type { Component, JSX } from "solid-js";

export const RefsView = lazy(() => import("./RefsView"));
export const BlameView = lazy(() => import("./BlameView"));
export const FileHistory = lazy(() => import("./FileHistory"));
export const ConflictResolver = lazy(() => import("./ConflictResolver"));
export const InteractiveRebase = lazy(() => import("./InteractiveRebase"));
export const RecomposeView = lazy(() => import("./RecomposeView"));
export const ExplainPanel = lazy(() => import("./ExplainPanel"));
export const WorktreesView = lazy(() => import("./WorktreesView"));
export const ReflogView = lazy(() => import("./ReflogView"));
export const BisectView = lazy(() => import("./BisectView"));
export const CustomCommandsView = lazy(() => import("./CustomCommandsView"));
export const SettingsView = lazy(() => import("./SettingsView"));
export const NotificationsView = lazy(() => import("./NotificationsView"));
export const LfsView = lazy(() => import("./LfsView"));
export const GitFlowView = lazy(() => import("./GitFlowView"));
export const SubmodulesView = lazy(() => import("./SubmodulesView"));
export const StashView = lazy(() => import("./StashView"));
export const AiView = lazy(() => import("./AiView"));

export const LazyViewFallback: Component = () => (
  <div role="status" style={{ padding: "16px", color: "var(--tx3)", "font-size": "12.5px" }}>
    Loading view...
  </div>
);

export const LazyViewBoundary: Component<{ children: JSX.Element }> = (props) => (
  <Suspense fallback={<LazyViewFallback />}>{props.children}</Suspense>
);
