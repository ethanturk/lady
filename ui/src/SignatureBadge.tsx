import { Show } from "solid-js";
import type { Component } from "solid-js";
import type { SignatureStatus } from "./commands";

/** Visual style per signature status. `None` renders nothing. */
const STYLE: Record<Exclude<SignatureStatus, "None">, { bg: string; fg: string; label: string; title: string }> = {
  Good: { bg: "#dafbe1", fg: "#1a7f37", label: "✓ signed", title: "Valid signature from a trusted key" },
  Untrusted: { bg: "#fff8c5", fg: "#9a6700", label: "✓ untrusted", title: "Valid signature, untrusted/expired key" },
  Bad: { bg: "#ffebe9", fg: "#cf222e", label: "✗ bad sig", title: "Bad / invalid signature" },
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
