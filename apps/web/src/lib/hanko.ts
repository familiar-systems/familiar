import { Hanko } from "@teamhanko/hanko-elements";

export const hankoApiUrl = import.meta.env.VITE_HANKO_API_URL;
export const hanko = new Hanko(hankoApiUrl);

export function getSessionToken(): string {
  return hanko.getSessionToken();
}
