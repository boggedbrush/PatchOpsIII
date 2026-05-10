import type { ReactNode } from "react";

type ControlRowProps = {
  title: string;
  detail: string;
  action: ReactNode;
};

export function ControlRow({ title, detail, action }: ControlRowProps) {
  return (
    <div className="control-row">
      <div>
        <h3>{title}</h3>
        <p>{detail}</p>
      </div>
      {action}
    </div>
  );
}
