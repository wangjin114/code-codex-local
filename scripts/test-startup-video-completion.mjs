import assert from 'node:assert/strict';
import {buildSync} from '../packages/explorer-ui/node_modules/esbuild/lib/main.js';
import {readFileSync,writeFileSync,mkdirSync} from 'node:fs';
import {resolve} from 'node:path';
import {createServer} from 'node:http';
import {launchFixture,delay} from './browser-fixture.mjs';

const root=resolve(import.meta.dirname,'..');
const source=buildSync({stdin:{contents:"import {mountStartupTransition} from './startup-transition';window.mountTransition=mountStartupTransition;",resolveDir:resolve(root,'packages/explorer-ui/src'),loader:'ts'},bundle:true,write:false,format:'iife',define:{__CODE_CODEX_STARTUP_TRANSITION_CSS__:JSON.stringify(readFileSync(resolve(root,'packages/explorer-ui/src/startup-transition.css'),'utf8'))}}).outputFiles[0].text;
const server=createServer((req,res)=>{res.setHeader('Content-Type','text/html');res.end('<!doctype html><body></body>');});
await new Promise(r=>server.listen(0,'127.0.0.1',r));
let browser;
const report={checks:[]};
try{
  browser=await launchFixture(`http://127.0.0.1:${server.address().port}/`);
  await browser.evaluate(source);
  // Exercise a real decoder and accelerated media clock, rather than synthetic timeupdate events.
  await browser.evaluate(`(async()=>{
    const canvas=document.createElement('canvas');canvas.width=160;canvas.height=90;
    const ctx=canvas.getContext('2d');const stream=canvas.captureStream(30);
    const recorder=new MediaRecorder(stream,{mimeType:'video/webm;codecs=vp8'});const chunks=[];
    recorder.ondataavailable=e=>chunks.push(e.data);
    const done=new Promise(r=>recorder.onstop=r);let frame=0;
    const interval=setInterval(()=>{ctx.fillStyle=frame++%2?'#2196f3':'#ff9800';ctx.fillRect(0,0,160,90);},30);
    recorder.start();await new Promise(r=>setTimeout(r,1800));recorder.stop();await done;
    clearInterval(interval);stream.getTracks().forEach(t=>t.stop());
    window.videoUrl=URL.createObjectURL(new Blob(chunks,{type:'video/webm'}));
    window.snapshot=()=>{const host=document.querySelector('.code-codex-startup-host');const video=host?.shadowRoot.querySelector('video');const overlay=host?.shadowRoot.querySelector('.codex-startup');return {phase:controller.phase,time:video?.currentTime,paused:video?.paused,opacity:overlay?Number(getComputedStyle(overlay).opacity):null};};
  })()`);
  for(const {earlyReady,clipEnd,waitMs,minimumVisiblePercent} of [
    {earlyReady:false,clipEnd:1.1,waitMs:760,minimumVisiblePercent:0},
    {earlyReady:true,clipEnd:1.1,waitMs:760,minimumVisiblePercent:100},
    {earlyReady:false,clipEnd:3,waitMs:1400,minimumVisiblePercent:0},
  ]){
    await browser.evaluate(`(async()=>{window.controller=mountTransition({target:document.body,videoSrc:videoUrl,clipStart:.2,clipEnd:${clipEnd},playbackRate:1.65,minimumVisiblePercent:${minimumVisiblePercent},fadePercent:100});await controller.mediaReady;controller.markRevealed();${earlyReady?'controller.signalReady();':''}})()`);
    if(earlyReady){
      const before=await browser.evaluate('snapshot()');
      assert.equal(before.phase,'loading','native readiness must not truncate the clip');
      report.checks.push({name:'early readiness waits for clip completion',value:before});
    }
    await delay(waitMs);
    const held=await browser.evaluate('snapshot()');
    assert.ok(clipEnd===1.1 ? held.time>=1.1&&held.time<1.18 : held.time>=1.6&&held.time<2.1,`clip must hold its last frame at 1.65x: ${JSON.stringify(held)}`);
    assert.equal(held.paused,true,'clip must stop instead of looping');
    assert.equal(held.phase,earlyReady?'exiting':'loading');
    if(!earlyReady)await browser.evaluate('controller.signalReady()');
    await delay(100);
    const fading=await browser.evaluate('snapshot()');
    assert.equal(fading.phase,'exiting');assert.equal(fading.time,held.time,'fade must keep the final frame');
    assert.ok(fading.opacity>0&&fading.opacity<1,'held final frame must visibly fade');
    await delay(clipEnd===1.1?600:1800);
    assert.equal(await browser.evaluate('controller.phase'),'complete');
    assert.equal(await browser.evaluate("document.querySelector('.code-codex-startup-host')===null"),true);
    report.checks.push({name:clipEnd===3?'natural media end holds final frame':earlyReady?'early readiness fades held frame after minimum':'late readiness never replays clip',held,fading});
  }
  await browser.evaluate('URL.revokeObjectURL(videoUrl)');
}catch(error){report.failure=error.stack;throw error;}
finally{await browser?.close();await new Promise(r=>server.close(r));mkdirSync(resolve(root,'artifacts/startup-video-completion'),{recursive:true});writeFileSync(resolve(root,'artifacts/startup-video-completion/result.json'),JSON.stringify(report,null,2));}
console.log(JSON.stringify(report));
