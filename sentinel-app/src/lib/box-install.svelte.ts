// Hand-off for the one-time "box created" payload from the create form to the dedicated
// install view (/settings/boxes/install). The box token is returned by the API only once
// and is never persisted, so it lives here in memory just long enough to render the install
// page — it's gone on reload, which is why the view redirects back to the list if empty.
export type PendingInstall = {
  /** Needed to mint the box's pairing code on the install view (card #81). */
  id: string;
  name: string;
  token: string;
  command: string;
  auto: boolean; // the box token was auto-minted (transparent enrollment)
  needsKey: boolean; // no auto key and no manual key → the command has a placeholder
};

export const pendingInstall = $state<{ value: PendingInstall | null }>({ value: null });
