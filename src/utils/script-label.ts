export interface NamedScript {
  name: string;
  display_name?: string | null;
}

export function scriptLabel(script: NamedScript): string {
  return script.display_name ?? script.name;
}

/** Include the stable name wherever an operation needs to be unambiguous. */
export function scriptTitle(script: NamedScript): string {
  const label = scriptLabel(script);
  return label === script.name ? label : `${label} (${script.name})`;
}

export function scriptLabelFor(scripts: readonly NamedScript[], name: string): string {
  const script = scripts.find((item) => item.name === name);
  return script ? scriptLabel(script) : name;
}
