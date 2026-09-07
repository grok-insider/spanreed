import * as React from "react";
import type { MigrationReview } from "./contracts";

export function MigrationReviewDetails({ review }: { review: MigrationReview }) {
  return <section aria-label="Migration review" className="fb-form">
    <p><strong>From</strong> <code>{review.sourceEnvironment}</code><br /><strong>To</strong> <code>{review.destinationEnvironment}</code></p>
    <ul>{review.items.map(item => <li key={item.sourceId} style={{overflowWrap:"anywhere"}}>
      <strong>{item.sourceId}</strong> → <strong>{item.provider}/{item.targetAlias}</strong>
      <p className="fb-muted">{item.action === "copyApiKey" ? "Copy API key. The imported account starts inactive." : "Authorize a new OAuth connection in the destination. Existing tokens are never copied."}</p>
    </li>)}</ul>
    <p className="fb-muted">Source accounts remain available. Imported accounts do not change your active account or routing.</p>
  </section>;
}
