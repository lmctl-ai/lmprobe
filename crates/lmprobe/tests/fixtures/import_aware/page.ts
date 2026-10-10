import { ImportedThing } from "./model";

export function renderThing(value: ImportedThing): string {
  return value.id;
}
