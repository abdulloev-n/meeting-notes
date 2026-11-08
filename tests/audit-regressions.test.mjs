import test from 'node:test';
import assert from 'node:assert/strict';
import {DatabaseSync} from 'node:sqlite';
import {readFileSync} from 'node:fs';
const storage=readFileSync(new URL('../src-tauri/src/storage.rs',import.meta.url),'utf8');
const extras=readFileSync(new URL('../src-tauri/src/extras.rs',import.meta.url),'utf8');
function migration(source){return source.slice(source.indexOf('fn migrate(')).match(/execute_batch\(\s*"([\s\S]*?)"\s*\)/)[1];}
function fixture(){
 const db=new DatabaseSync(':memory:');db.exec(migration(storage));
 const insert=db.prepare('INSERT INTO meetings VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)');
 for(const id of ['legacy-1','legacy-2','assigned'])insert.run(id,id,null,'2026-01-01','2026-01-01','2026-01-01',60,'audio/'+id+'.m4a','original text','original summary','[]','[]','READY',null,'original-model','original-summary-model');
 db.prepare('INSERT INTO action_items VALUES(?,?,?,?,?,?)').run('completed-task','legacy-1','Keep completion','Owner','Friday',1);
 db.prepare('INSERT INTO settings VALUES(?,?)').run('app','{"transcriptionModel":"gpt-4o-mini-transcribe"}');
 return db;
}
const countQuery=extras.match(/let mut st=c.prepare\("([^"]+)"\)/)[1];
test('legacy meetings count as Unsorted before metadata exists',()=>{
 const db=fixture();db.exec(migration(extras));
 let counts=db.prepare(countQuery).all();assert.equal(counts.find(x=>x.name==='Unsorted')[Object.keys(counts[0])[2]],3);
 db.prepare('INSERT INTO meeting_extras(meeting_id,collection_name) VALUES(?,?)').run('assigned','Personal');
 counts=db.prepare(countQuery).all();const count=row=>row[Object.keys(row)[2]];
 assert.equal(count(counts.find(x=>x.name==='Unsorted')),2);assert.equal(count(counts.find(x=>x.name==='Personal')),1);db.close();
});
test('additive metadata migrations preserve old notes, task IDs, completion and settings',()=>{
 const db=fixture();const before={meetings:db.prepare('SELECT * FROM meetings').all(),tasks:db.prepare('SELECT * FROM action_items').all(),settings:db.prepare('SELECT * FROM settings').all()};
 db.exec(migration(extras));db.exec(migration(extras));
 assert.deepEqual(db.prepare('SELECT * FROM meetings').all(),before.meetings);assert.deepEqual(db.prepare('SELECT * FROM action_items').all(),before.tasks);assert.deepEqual(db.prepare('SELECT * FROM settings').all(),before.settings);assert.equal(db.prepare('PRAGMA integrity_check').get().integrity_check,'ok');db.close();
});
test('startup does not recreate a default collection the user deleted',()=>{
 const db=fixture();db.exec(migration(extras));
 db.prepare("DELETE FROM collections WHERE name='Personal'").run();
 const before=db.prepare('SELECT * FROM collections').all();
 db.exec(migration(extras));
 assert.deepEqual(db.prepare('SELECT * FROM collections').all(),before);db.close();
});
test('read-only cache query returns a miss without creating metadata; stale signatures miss',()=>{
 const db=fixture();db.exec(migration(extras));const source=extras.slice(extras.indexOf('fn cached_analysis('));
 const query=source.match(/"(SELECT a.result_json[^"]+)"/)[1];
 assert.equal(db.prepare(query).get('legacy-1','sig','model','English'),undefined);assert.equal(db.prepare('SELECT COUNT(*) n FROM meeting_extras').get().n,0);
 db.prepare('INSERT INTO meeting_extras(meeting_id,detected_mode) VALUES(?,?)').run('legacy-1','Meeting');
 db.prepare('INSERT INTO analysis_cache VALUES(?,?,?,?,?,?)').run('legacy-1','Meeting','sig','model','English','{"mode":"Meeting"}');
 assert.equal(db.prepare(query).get('legacy-1','sig','model','English').result_json,'{"mode":"Meeting"}');assert.equal(db.prepare(query).get('legacy-1','new-sig','model','English'),undefined);db.close();
});
