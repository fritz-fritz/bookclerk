import { validate } from "email-validator";

/**
 * True when `value` is empty (optional) or a library-validated email address.
 *
 * Empty / whitespace is allowed so contact email can be cleared. Non-empty
 * values must pass `email-validator` (rejects `user@gmail` without a TLD).
 *
 * @param value - Raw input from an email field.
 * @returns Whether the field can be submitted.
 */
export function isOptionalEmailValid(value: string): boolean {
  const trimmed = value.trim();
  if (!trimmed) return true;
  return validate(trimmed);
}
