export function openDatabase(name) {
  return new Promise((resolve, reject) => {
    const request = indexedDB.open(name, 1);
    let blocked = false;
    request.onupgradeneeded = () => {
      for (const store of ["blobs", "actions"]) {
        request.result.createObjectStore(store).createIndex("size", "size");
      }
    };
    request.onerror = () => reject(request.error);
    request.onblocked = () => {
      blocked = true;
      reject(new Error("IndexedDB upgrade is blocked; close other connections"));
    };
    request.onsuccess = () => {
      const db = request.result;
      if (blocked) return db.close();
      db.onversionchange = () => db.close();
      resolve(db);
    };
  });
}

export function transaction(db, stores, mode, work) {
  return new Promise((resolve, reject) => {
    const tx = db.transaction(stores, mode);
    let result;
    tx.oncomplete = () => resolve(result);
    tx.onabort = () => reject(tx.error ?? new Error("IndexedDB transaction aborted"));
    try {
      work(tx, (value) => { result = value; });
    } catch (error) {
      tx.abort();
      reject(error);
    }
  });
}

export function measure(store, done) {
  let count = 0;
  let size = 0;
  store.index("size").openKeyCursor().onsuccess = (event) => {
    const cursor = event.target.result;
    if (!cursor) return done(count, size);
    count++;
    size += cursor.key;
    cursor.continue();
  };
}
