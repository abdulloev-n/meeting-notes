import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync,writeFileSync} from 'node:fs';
import ts from 'typescript';
const stub=code=>'data:text/javascript;base64,'+Buffer.from(code).toString('base64');
let source=readFileSync(new URL('../src/export.ts',import.meta.url),'utf8');
source=source.replace("'./api'",JSON.stringify(stub('export const api={saveExport:async(name,extension,bytes)=>{globalThis.auditExport={name,extension,bytes};return "test"}}')));
source=source.replace("'./types'",JSON.stringify(stub('export const titleOf=m=>m.customTitle||m.generatedTitle;export const formatDate=()=>"2026-11-07";export const formatDuration=()=>"12 seconds";')));
const built=ts.transpileModule(source,{compilerOptions:{target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.ESNext}}).outputText;
const {exportMeeting}=await import(stub(built));
const meeting={generatedTitle:'Audit <meeting> & notes',startedAt:'2026-11-07',durationSeconds:12,summary:'Original summary',decisions:['Friday'],importantDetails:['Original detail'],actionItems:[{task:'Keep saved task',owner:'Sam',deadline:'Friday',completed:true}],transcript:'Original transcript'};
test('DOCX central directory matches local files and preserves saved tasks',async()=>{
 await exportMeeting(meeting,{summary:'New summary',decisions:[],details:[],actionItems:[{task:'Unaccepted suggestion'}]},undefined,'docx');
 const b=Buffer.from(globalThis.auditExport.bytes);const end=b.length-22;
 assert.equal(b.readUInt32LE(end),0x06054b50);const count=b.readUInt16LE(end+10);assert.equal(count,3);
 let at=b.readUInt32LE(end+16);const entries=new Map();
 for(let i=0;i<count;i++){
  assert.equal(b.readUInt32LE(at),0x02014b50);const nameLength=b.readUInt16LE(at+28),extraLength=b.readUInt16LE(at+30),commentLength=b.readUInt16LE(at+32);
  const name=b.subarray(at+46,at+46+nameLength).toString();const local=b.readUInt32LE(at+42);
  assert.equal(b.readUInt32LE(local),0x04034b50);const localNameLength=b.readUInt16LE(local+26),localExtraLength=b.readUInt16LE(local+28);
  assert.equal(b.subarray(local+30,local+30+localNameLength).toString(),name);
  const start=local+30+localNameLength+localExtraLength;entries.set(name,b.subarray(start,start+b.readUInt32LE(at+24)).toString());at+=46+nameLength+extraLength+commentLength;
 }
 assert.equal(at,end);assert.deepEqual([...entries.keys()],['[Content_Types].xml','_rels/.rels','word/document.xml']);
 const doc=entries.get('word/document.xml');assert.ok(doc.includes('Audit &lt;meeting&gt; &amp; notes'));assert.ok(doc.includes('Keep saved task'));assert.ok(!doc.includes('Unaccepted suggestion'));
 if(process.env.AUDIT_DOCX_PATH)writeFileSync(process.env.AUDIT_DOCX_PATH,b);
});
