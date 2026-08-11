/**
 * The product's name **as customers see it** (card #76).
 *
 * The codebase, the routes, the components and the repo stay "Sentinel" — renaming those buys
 * nothing and breaks deploy paths. Only the COPY is Olhar AI: anything a reader can see.
 *
 * One constant, because spelling it out did not survive contact with time. Eighteen strings across
 * eight message files were still saying "Sentinel" long after the rename — including the billing
 * trial note, which a customer reads at the exact moment they are deciding whether to pay us.
 *
 * Mirrors `app_core::brand::PRODUCT` on the backend. If the name ever changes again, these two
 * lines are the change.
 */
export const PRODUCT = "Olhar AI";
