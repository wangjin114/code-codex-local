import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {homeFixture, openBrowser, record, sleep} from './fixtures/browser-fixture.mjs';
const treeTag='code-codex-v'+JSON.parse(readFileSync(new URL('../packages/explorer-ui/package.json',import.meta.url),'utf8')).version.replaceAll('.','-');

// Real production UI and keyboard events, with faults at the native bridge boundary.
// No watcher notifications: recovery must not rely on their delivery.
async function scenario(mode) {
  const setup = `
window.mode=${JSON.stringify(mode)};
window.entries=['a.txt','b.txt'];window.destination=[];window.mutations=[];window.listings=[];
window.currentThread='thread_home_001';window.hold=false;window.releaseAction=()=>{};
window.directoryHeld=false;window.releaseDirectory=()=>{};window.retryDelete=false;
const requestBeforeFixture=window.__codeCodex.request;
window.__codeCodex.request=async req=>{
 const {method,params}=req;
 if(method==='explorer.context')window.currentThread=params.threadId;
 if(method==='explorer.list'){
  listings.push({thread:currentThread,path:params.relativePath});
  const names=currentThread==='thread_home_001'?entries:['new.txt'];
  const result={entries:params.relativePath==='dst'?destination.map(name=>({name,relativePath:'dst/'+name,kind:'file'})):
   [{name:'dst',relativePath:'dst',kind:'directory'},...names.map(name=>({name,relativePath:name,kind:'file'}))]};
  if(mode==='copy-load-race'&&params.relativePath==='dst'&&!directoryHeld){directoryHeld=true;await new Promise(resolve=>releaseDirectory=resolve);}
  return result;
 }
 if(method==='explorer.entry.delete'){
  mutations.push({method,path:params.relativePath,thread:currentThread});
  if(!entries.includes(params.relativePath))throw Object.assign(new Error('Not found'),{code:'NOT_FOUND'});
  if(mode==='delete-partial'&&params.relativePath==='b.txt'&&!retryDelete)throw Object.assign(new Error('Permission denied'),{code:'ACCESS_DENIED'});
  entries=entries.filter(name=>name!==params.relativePath);
  if(mode==='delete-context-switch'&&!hold){hold=true;await new Promise(resolve=>releaseAction=resolve);}
  return {deleted:true};
 }
 if(method==='explorer.entry.copy'||method==='explorer.entry.move'){
  mutations.push({method,paths:params.sourcePaths,thread:currentThread});
  const names=params.sourcePaths;
  for(const name of names){
   if(mode.endsWith('partial')&&name==='b.txt')throw Object.assign(new Error('Already exists'),{code:'CONFLICT'});
   destination.push(name);if(method==='explorer.entry.move')entries=entries.filter(entry=>entry!==name);
  }
  if(mode==='copy-load-race')setTimeout(()=>releaseDirectory(),150);
  return {entries:names.map(name=>({name,relativePath:'dst/'+name,kind:'file'}))};
 }
 return requestBeforeFixture(req);
};
`;
  const html=homeFixture().replace('const errors=[];',setup+'const errors=[];');
  const browser=await openBrowser(html);
  try {
    await browser.wait(`document.querySelector('${treeTag}')?.dataset.state==='ready'`,'tree ready');
    await browser.ev(`window.tree=document.querySelector('${treeTag}');tree.collapse(false);
      window.key=(key,ctrl=false)=>tree.shadowRoot.querySelector('.tree-shell').dispatchEvent(new KeyboardEvent('keydown',{key,ctrlKey:ctrl,bubbles:true,cancelable:true}));
      window.select=(path,ctrl=false)=>tree.shadowRoot.querySelector('[role=treeitem][data-path="'+path+'"]').dispatchEvent(new MouseEvent('click',{bubbles:true,ctrlKey:ctrl}));
      select('a.txt');select('b.txt',true);window.beforeLists=listings.length;`);
    if(mode.startsWith('delete')) {
      await browser.ev(`key('Delete');`);
      if(mode==='delete-context-switch') {
        await browser.wait('hold','first deletion held');
        await browser.ev(`document.querySelector('[data-app-action-sidebar-thread-id]').dataset.appActionSidebarThreadId='local:thread_home_002';document.querySelector('[data-above-composer-conversation-id]').dataset.aboveComposerConversationId='thread_home_002';document.querySelector('[data-response-annotation-conversation]').dataset.responseAnnotationConversation='thread_home_002';`);
        await browser.wait(`currentThread==='thread_home_002'&&tree.dataset.state==='ready'`,'new workspace ready');
        await browser.ev('releaseAction();');
      }
    } else {
      await browser.ev(`key(${JSON.stringify(mode.startsWith('copy')?'c':'x')},true);select('dst');key('ArrowRight');`);
      await browser.wait(`listings.some(x=>x.path==='dst')`,'destination expanded');
      await browser.ev(`key('v',true);`);
    }
    await browser.wait('mutations.length>0','mutation dispatched');
    await sleep(900);
    const observed=await browser.ev(`({rows:[...tree.shadowRoot.querySelectorAll('[role=treeitem][data-path]')].map(e=>e.dataset.path),
      selected:[...tree.shadowRoot.querySelectorAll('[aria-selected=true][data-path]')].map(e=>e.dataset.path),
      notice:[...tree.shadowRoot.querySelectorAll('[role=status]')].map(e=>e.textContent).join(' '),
      listings:listings.slice(beforeLists),mutations,entries,destination,errors})`);
    assert.deepEqual(observed.errors,[]);
    if(mode==='delete-context-switch') {
      assert.deepEqual(observed.mutations.map(x=>x.thread),['thread_home_001'],'old deletion must stop after a workspace switch');
      assert.ok(observed.rows.includes('new.txt'));
      assert.ok(!observed.notice.includes('Permission'));
    } else if(mode==='delete-partial') {
      assert.ok(!observed.rows.includes('a.txt'),'completed deletion must disappear after a partial failure');
      assert.ok(observed.rows.includes('b.txt'),'failed deletion must remain');
      assert.deepEqual(observed.selected,['b.txt'],'only remaining files should stay selected for retry');
      assert.ok(observed.notice,'failure must remain visible');
      await browser.ev(`retryDelete=true;key('Delete');`);
      await sleep(400);
      assert.deepEqual(await browser.ev('entries'),[],'retry should delete only the remaining file');
      assert.deepEqual(await browser.ev('mutations.map(x=>x.path)'),['a.txt','b.txt','b.txt']);
    } else if(mode==='delete-success') {
      assert.ok(!observed.rows.includes('a.txt')&&!observed.rows.includes('b.txt'),'root deletions must refresh');
    } else {
      assert.ok(observed.rows.includes('dst/a.txt'),'completed paste must appear');
      if(mode.startsWith('move'))assert.ok(!observed.rows.includes('a.txt'),'move must refresh source directory');
      if(mode.endsWith('partial')){assert.ok(observed.rows.includes('b.txt'));assert.ok(observed.notice);}
      else assert.ok(observed.rows.includes('dst/b.txt'));
    }
    return {mode,passed:true,observed};
  } finally {await browser.close();}
}

const results=[];
for(const mode of ['delete-success','delete-partial','move-success','move-partial','copy-partial','copy-load-race','delete-context-switch']) {
  try {results.push(await scenario(mode));console.log(mode+' passed');}
  catch(error){results.push({mode,passed:false,error:String(error)});console.error(mode+': '+error);}
}
record('keyboard-file-actions.json',{results});
assert.ok(results.every(result=>result.passed),'keyboard file action regressions');
