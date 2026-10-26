import { record, str } from "./decode.ts";

export interface StoreApp {
  id: string;
  name: string;
  publisher: string;
  version: string;
}

export function storeApp(value: unknown, where: string): StoreApp {
  const item = record(value, where);
  return {
    id: str(item["id"], `${where}.id`),
    name: str(item["name"], `${where}.name`),
    publisher: str(item["publisher"], `${where}.publisher`),
    version: str(item["version"], `${where}.version`),
  };
}
