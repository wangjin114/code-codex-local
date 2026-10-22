import assert from 'node:assert/strict';
import {createServer} from 'node:http';
import {build} from '../packages/explorer-ui/node_modules/esbuild/lib/main.js';
import {launchFixture} from './browser-fixture.mjs';
const output=await build({entryPoints:['packages/explorer-ui/src/features/workspace/file-actions.ts'],bundle:true,write:false,format:'iife',globalName:'FileActionsRecovery',platform:'browser',target:'chrome120',define:{__CODE_CODEX_VERSION__:'"0.5.7"'}});
const bundle=output.outputFiles[0].contents;
const server=createServer((req,res)=>{res.setHeader('Content-Type',req.url==='/controller.js'?'text/javascript':'text/html');res.end(req.url==='/controller.js'?bundle:'<!doctype html><script src="/controller.js"></script>');});
await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
let browser;
try{
 browser=await launchFixture('http://127.0.0.1:'+server.address().port+'/');
 for(const refreshFailure of [false,true]){
  const result=await browser.evaluate(`(async()=>{
   const selected=new Set(['a.txt','b.txt']),closed=[],refreshed=[],notices=[];let released=0;
   const context={threadId:'thread-one'},bridge={available:true,request:async(method,params)=>{if(params.relativePath==='b.txt')throw Error('Second delete failed')}};
   const host={workspaceSession:{context,bridge,generation:1,threadId:'thread-one'},canUseBridge:()=>true,selectedPaths:selected,mutations:{acquire:()=>({release(){released++}})},
    clearSelection(){selected.clear()},closePreviewTabsWithin(path){closed.push(path)},removeCommittedSelection(path){selected.delete(path)},
    model:{hasLoaded:()=>false},loadDirectory:async(path)=>{refreshed.push(path);if(${refreshFailure})throw Error('Refresh failed')},showActionNotice(message){notices.push(message)}};
   let error;try{await new FileActionsRecovery.FileActionsController(host).deleteSelectedFiles()}catch(e){error=e.message}
   return {selected:[...selected],closed,refreshed,released,error};
  })()`);
  assert.deepEqual(result.closed,['a.txt'],'committed delete closes its preview despite a later failure');
  assert.deepEqual(result.selected,['b.txt'],'only uncommitted entries remain selected');
  assert.deepEqual(result.refreshed,[''],'root listing refreshes after partial failure');
  assert.equal(result.released,1,'refresh failure cannot retain the mutation lease');
 }
 console.log('File action recovery passed: partial deletion, selection, root refresh and lease release.');
}finally{await browser?.close();await new Promise(resolve=>server.close(resolve));}
