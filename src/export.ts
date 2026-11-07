import { api } from './api';
import { formatDate, formatDuration, titleOf, type Analysis, type Meeting, type MeetingExtra } from './types';

const escapeXml = (v:unknown) => String(v ?? '').replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;').replace(/"/g,'&quot;').replace(/'/g,'&apos;');
const time = (s:number) => [Math.floor(s/60),Math.floor(s%60)].map(v=>String(v).padStart(2,'0')).join(':');
function content(m:Meeting,a:Analysis|null,e?:MeetingExtra) {
 return {title:titleOf(m),date:formatDate(m.startedAt),duration:formatDuration(m.durationSeconds),
  summary:a?.summary||m.summary||'',
  decisions:a?.decisions||m.decisions,details:a?.details||m.importantDetails,
  actions:m.actionItems.map(x=>({task:x.task,owner:x.owner,deadline:x.deadline})),
  transcript:e?.segments?.length?e.segments.map(s=>time(s.start)+' · '+(e.speakerNames[s.speaker]||s.speaker)+': '+s.text).join('\n\n'):(m.transcript||'')};
}
function markdown(m:Meeting,a:Analysis|null,e?:MeetingExtra){
 const c=content(m,a,e);return '# '+c.title+'\n\n'+c.date+' · '+c.duration+'\n\n'
 +'## Summary\n\n'+c.summary+'\n\n## Key decisions\n\n'+c.decisions.map(x=>'- '+x).join('\n')
 +'\n\n## Action items\n\n'+c.actions.map(x=>'- '+x.task+(x.owner?' ('+x.owner+')':'')+(x.deadline?' · '+x.deadline:'')).join('\n')
 +'\n\n## Details\n\n'+c.details.map(x=>'- '+x).join('\n')
 +'\n\n## Transcript\n\n'+c.transcript+'\n';
}
const u16=(n:number)=>[n&255,(n>>>8)&255];const u32=(n:number)=>[n&255,(n>>>8)&255,(n>>>16)&255,(n>>>24)&255];
function crc32(bytes:Uint8Array){let c=-1;for(const b of bytes){c^=b;for(let k=0;k<8;k++)c=(c>>>1)^(-(c&1)&0xEDB88320)}return(c^-1)>>>0}
function zipFiles(files:Record<string,string>){
 const enc=new TextEncoder(),local:Uint8Array[]=[],central:Uint8Array[]=[];let offset=0;
 for(const [name,body] of Object.entries(files)){
  const nam=enc.encode(name),data=enc.encode(body),crc=crc32(data);
  const head=new Uint8Array([80,75,3,4,20,0,0,0,0,0,0,0,0,0,...u32(crc),...u32(data.length),...u32(data.length),...u16(nam.length),0,0,...nam]);
  local.push(head,data);
  central.push(new Uint8Array([80,75,1,2,20,0,20,0,0,0,0,0,0,0,0,0,...u32(crc),...u32(data.length),...u32(data.length),...u16(nam.length),0,0,0,0,0,0,0,0,0,0,0,0,...u32(offset)]),nam);
  offset+=head.length+data.length;
 }
 const centralSize=central.reduce((sum,a)=>sum+a.length,0);
 const end=new Uint8Array([80,75,5,6,0,0,0,0,...u16(Object.keys(files).length),...u16(Object.keys(files).length),...u32(centralSize),...u32(offset),0,0]);
 return new Blob([...local,...central,end] as BlobPart[],{type:'application/vnd.openxmlformats-officedocument.wordprocessingml.document'});
}
function makeDocx(m:Meeting,a:Analysis|null,e?:MeetingExtra){
 const c=content(m,a,e);
 const p=(v:string,bold=false)=>'<w:p><w:r>'+(bold?'<w:rPr><w:b/></w:rPr>':'')+'<w:t xml:space="preserve">'+escapeXml(v)+'</w:t></w:r></w:p>';
 const sections:[string,string[]][]=[
  ['Summary',[c.summary]],['Key decisions',c.decisions],
  ['Action items',c.actions.map(x=>x.task+(x.owner?' · '+x.owner:'')+(x.deadline?' · '+x.deadline:''))],
  ['Details',c.details],['Transcript',c.transcript.split(/\n+/).filter(Boolean)]
 ];
 const document='<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>'
 +p(c.title,true)+p(c.date+' · '+c.duration)+sections.map(([h,lines])=>p(h,true)+(lines.length?lines.map(x=>p(x)).join(''):p('None'))).join('')+'<w:sectPr/></w:body></w:document>';
 return zipFiles({
 '[Content_Types].xml':'<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>',
 '_rels/.rels':'<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>',
 'word/document.xml':document});
}
export async function exportMeeting(m:Meeting,a:Analysis|null,e:MeetingExtra|undefined,format:'md'|'docx'|'pdf'){
 const c=content(m,a,e);
 if(format==='pdf'){
   document.getElementById('meeting-print')?.remove();
   const wrap=document.createElement('article');wrap.id='meeting-print';wrap.className='print-document';
   const row=(heading:string,value:string)=>{const sec=document.createElement('section');const h=document.createElement('h2');h.textContent=heading;const p=document.createElement('p');p.style.whiteSpace='pre-wrap';p.textContent=value;sec.append(h,p);wrap.appendChild(sec)};
   const title=document.createElement('h1');title.textContent=c.title;const meta=document.createElement('p');meta.textContent=c.date+' · '+c.duration;wrap.append(title,meta);
   row('Summary',c.summary);row('Key decisions',c.decisions.map(x=>'• '+x).join('\n'));row('Action items',c.actions.map(x=>'• '+x.task+(x.owner?' · '+x.owner:'')+(x.deadline?' · '+x.deadline:'')).join('\n'));row('Details',c.details.map(x=>'• '+x).join('\n'));row('Transcript',c.transcript);
   document.body.appendChild(wrap);window.addEventListener('afterprint',()=>wrap.remove(),{once:true});window.print();return;
 }
 const data=format==='md'?new Blob([markdown(m,a,e)],{type:'text/markdown;charset=utf-8'}):makeDocx(m,a,e);
 await api.saveExport(c.title,format,Array.from(new Uint8Array(await data.arrayBuffer())));
}
