// Nervape Headwear (the cluster the user reviewed, Main.dc.html): real API answers captured from
// :8101 on 2026-11-03 — the cluster's insights (a DOB/0 cluster whose one decoder step has no byte
// mapping in the API: binary 0x1c84…7b5f is not a reviewed dialect), its stored description (which
// declares the pattern itself) and the genetics of its newest object 0x2065ba12…acee.
import type { CollectionInsights, ObjectGenetics } from '@/lib/api';
import insights from '@/__tests__/fixtures/objects/nervape-headwear-insights.json';
import genetics from '@/__tests__/fixtures/objects/nervape-headwear-object-genetics.json';

export const HEADWEAR_CLUSTER =
  '0x1a2ca0c10daadb9f9e087283fecdc72c136dfd0d050d8041f2d4a4cf012d5149';
export const HEADWEAR_OBJECT = '0x2065ba12a3985000f54eb6818bd43e0694dbbc2fb9c5c3d08317895a1012acee';
export const headwearDescription =
  '{"description":"Headwear for a Nervape isn\'t just a fashion statement, it can also shield you from the rocks life throws at you.","dob":{"ver":0,"decoder":{"type":"code_hash","hash":"0x1c84212ebd817e9de09d2a79f85cc421b684eda63409cfa75688f98716e77b5f"},"pattern":[["prev.type","string",0,1,"options",["image"]],["prev.bg","string",0,74,"utf8"],["prev.view","number",74,1,"range",[0,255]],["Entity","number",75,8,"raw"]]}}';
export const headwearInsights = insights as CollectionInsights;
export const headwearGenetics = genetics as ObjectGenetics;
