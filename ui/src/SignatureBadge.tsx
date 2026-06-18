import { Show } from "solid-js";
import type { Component } from "solid-js";
import type { SignatureStatus } from "./commands";

/** Visual style per signature status. `None` renders nothing. */
const STYLE: Record<Exclude<SignatureStatus, "None">, { bg: string; fg: string; label: string; title: string }> = {
  Good: { bg: "var(--success-bg)", fg: "var(--success)", label: "✓ signed", title: "Valid signature from a trusted key" },
  Untrusted: { bg: "var(--warning-bg)", fg: "var(--warning)", label: "✓ untrusted", title: "Valid signature, untrusted/expired key" },
  Bad: { bg: "var(--danger-bg)", fg: "var(--danger)", label: "✗ bad sig", title: "Bad / invalid signature" },
};

/** A small signature-verification badge for a commit (PH3-005). */
const SignatureBadge: Component<{ status: SignatureStatus | undefined }> = (props) => (
  <Show when={props.status && props.status !== "None"}>
    {(() => {
      const s = STYLE[props.status as Exclude<SignatureStatus, "None">];
      return (
        <span
          title={s.title}
          style={{
            background: s.bg,
            color: s.fg,
            "border-radius": "3px",
            padding: "0 0.3rem",
            "font-size": "0.68rem",
            "white-space": "nowrap",
            "flex-shrink": "0",
          }}
        >
          {s.label}
        </span>
      );
    })()}
  </Show>
);

export default SignatureBadge;
