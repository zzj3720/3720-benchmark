const fs = require('node:fs');
const vm = require('node:vm');
const ts = {transpile:s=>require('node:module').stripTypeScriptTypes(s),ScriptTarget:{ES2022:1}};
const path = require('node:path');
const source = fs.readFileSync(path.resolve(__dirname, '../../../observer-platform/app/page.tsx'),'utf8');
const start = source.indexOf('    if (paused) return;');
const end = source.indexOf('\n  }, [selectedId, paused]);',start);
const body = ts.transpile('(function(){'+source.slice(start,end)+'})()', {target:ts.ScriptTarget.ES2022});
function setup() {
 const requests=[];let stream;const state={};
 const ctx={paused:false,selectedId:'run1',URL,console:{warn(){}},gatewayUrl:p=>new URL('http://fixture'+p),EventSource:class{constructor(){stream=this} close(){}},fetch:()=>new Promise((resolve,reject)=>requests.push({resolve,reject})),hydrateRunAssets:async x=>x,reuseUnchangedState:(_p,n)=>n,setConnection:x=>state.connection=x,setSnapshotNow:x=>state.now=x,setRuns:x=>state.runs=x,setSelectedGame:x=>state.game=x,setDetail:f=>state.detail=f(state.detail)};
 vm.runInNewContext(body,ctx);
 const emit=(seq,live=true)=>stream.onmessage({data:JSON.stringify({generated_at:1000,revision:seq,runs:[{id:'run1',game:'parabox',latest_sequence:seq,live}]})});
 const answer=(i,seq)=>requests[i].resolve({ok:true,json:async()=>({run:{id:'run1',latest_sequence:seq,state:{}}})});
 return {requests,state,emit,answer,stream};
}
const drain=()=>new Promise(r=>setImmediate(r));
(async()=>{
 let h=setup();h.emit(1);h.emit(2);h.answer(1,2);await drain();h.answer(0,1);await drain();console.log('detail_out_of_order',JSON.stringify({expected_sequence:2,actual_sequence:h.state.detail.latest_sequence}));
 h=setup();h.emit(1);h.answer(0,1);await drain();h.emit(1,false);await drain();console.log('lease_change_same_sequence',JSON.stringify({detail_fetches:h.requests.length,summary_live:h.state.runs[0].live,detail_refetched:h.requests.length>1}));
 h=setup();h.emit(1);h.requests[0].reject(new Error('temporary failure'));await drain();await drain();console.log('failed_detail_no_new_events',JSON.stringify({fetches:h.requests.length,connection:h.state.connection,detail:h.state.detail??null}));
 h.stream.onmessage({data:JSON.stringify({error:'projection failed',revision:2})});console.log('sse_error_payload',JSON.stringify({connection:h.state.connection,run_count:h.state.runs.length}));
 const loadStart=source.indexOf('  async function loadAttemptReplay(');const loadEnd=source.indexOf('\n  async function exportSegment(',loadStart);
 const followStart=source.indexOf('  function followLive()');const followEnd=loadStart;
 const code=ts.transpile(source.slice(followStart,followEnd)+source.slice(loadStart,loadEnd),{target:ts.ScriptTarget.ES2022});
 let resolve;const replayState={};const c={runDetail:{id:'run1'},attemptReplay:null,frames:[],gatewayUrl:p=>new URL('http://fixture'+p),fetch:()=>new Promise(r=>resolve=r),setCursorKey:x=>replayState.cursor=x,setPlaying:x=>replayState.playing=x,setAttemptReplayLoading:x=>replayState.loading=x,setPendingAttemptReplay:x=>replayState.pending=x,setAttemptReplayError:x=>replayState.error=x,setAttemptReplay:x=>replayState.replay=x};
 vm.createContext(c);vm.runInContext(code,c);const p=c.loadAttemptReplay(8);c.followLive();resolve({ok:true,json:async()=>({attempt_id:8,frames:[{key:'f1'}]})});await p;console.log('return_live_during_replay_fetch',JSON.stringify({expected_replay:null,actual_attempt:replayState.replay?.attempt_id}));
})();
