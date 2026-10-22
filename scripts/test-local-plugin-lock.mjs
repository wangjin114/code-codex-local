import test from 'node:test';
import assert from 'node:assert/strict';
import {validateLock} from '../packages/explorer-ui/scripts/plugin-distribution.mjs';
const fixture=()=>({schema:1,packages:[{inputs:{'src/player.ts':'a'.repeat(64)},artifactRoot:'artifacts/local-plugin-assets/appearance/codex-startup-transition',descriptor:{id:'codex-startup-transition',category:'appearance',api:1,version:'1.1.26-local.1',releaseTag:'v0.5.7',localOnly:true,asset:'CodeCodex-plugin-codex-startup-transition-1.1.26-local.1.js',size:3,sha256:'b'.repeat(64),resources:[]}}]});
test('exact bundled local player passes the real distribution lock gate',()=>assert.equal(validateLock(fixture()).packages[0].descriptor.version,'1.1.26-local.1'));
test('local allowance rejects other identities, revisions and resource closures',()=>{
 for(const change of [{id:'other'},{version:'1.1.18-local.1'},{version:'1.1.26'},{localOnly:'true'},{resources:[{asset:'extra.js',size:1,sha256:'c'.repeat(64)}]}]){
  const lock=fixture();Object.assign(lock.packages[0].descriptor,change);assert.throws(()=>validateLock(lock),/locked plugin identity/);
 }
});
test('ordinary published identities still require exact version and asset integrity',()=>{
 const lock=fixture();Object.assign(lock.packages[0].descriptor,{localOnly:false,version:'1.1.26',asset:'CodeCodex-plugin-codex-startup-transition-1.1.26.js'});assert.doesNotThrow(()=>validateLock(lock));
 lock.packages[0].descriptor.sha256='corrupt';assert.throws(()=>validateLock(lock),/asset identity/);
});
