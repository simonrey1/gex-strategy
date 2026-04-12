export type BadgeVariant = "pw" | "cw" | "vf" | "wb" | "entry" | "exit";

interface BadgeProps {
  label: string;
  variant: BadgeVariant;
}

export function Badge({ label, variant }: BadgeProps) {
  return <span className={`badge badge-${variant}`}>{label}</span>;
}
