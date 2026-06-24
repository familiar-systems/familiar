// The HTTP-status -> human message mapping shared by the create and edit connectors.
// Both surface the same status set (409 / 422 / 404 / other), but the copy is specific
// to the operation (a 409 on create is "already exists"; on edit it is "already
// changed"), so the skeleton lives here and each caller passes its own wording.

export interface RelationshipErrorCopy {
  /** 409: the live-fact uniqueness conflict, in this operation's terms. */
  conflict: string;
  /** 422: the request was rejected as invalid, in this operation's terms. */
  unprocessable: string;
  /** 404: a referenced row or page is gone, in this operation's terms. */
  notFound: string;
  /** The verb in the catch-all ("Failed to {verb} relationship ({status})."). */
  verb: string;
}

export function relationshipErrorMessage(status: number, copy: RelationshipErrorCopy): string {
  switch (status) {
    case 409:
      return copy.conflict;
    case 422:
      return copy.unprocessable;
    case 404:
      return copy.notFound;
    default:
      return `Failed to ${copy.verb} relationship (${status}).`;
  }
}
