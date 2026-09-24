import type { ReactNode } from "react";
import { CircleCheck } from "lucide-react";
import { Alert, AlertAction, AlertDescription, AlertTitle } from "@fabrials/ui";

export function ErrorAlert({ title = "Something went wrong", error, action }: { title?: string; error: string | null | undefined; action?: ReactNode }) {
  if (!error) return null;
  return <Alert variant="destructive">
    <AlertTitle>{title}</AlertTitle>
    <AlertDescription>{error}</AlertDescription>
    {action && <AlertAction>{action}</AlertAction>}
  </Alert>;
}

export function Done({ children }: { children: ReactNode }) {
  if (!children) return null;
  return <p className="sr-done" role="status"><CircleCheck aria-hidden size={16} />{children}</p>;
}
