{-# LANGUAGE ForeignFunctionInterface #-}
{-# LANGUAGE ScopedTypeVariables #-}
{-# LANGUAGE TupleSections #-}

-- | Bindings for writing mira plugins in Haskell.
--
-- A plugin is a module that exports its setup function to C as @mira_hs_main@ (it can have
-- any Haskell name), built into a shared library together with
-- this module and @cbits/mira_hs.c@ (see @plugins/swirl@ and its build script):
--
-- > foreign export ccall "mira_hs_main" pluginMain :: Ptr () -> IO CInt
-- > pluginMain :: Ptr () -> IO CInt
-- > pluginMain = plugin $ \app -> do
-- >   transform <- lookupComponent app "mira.Transform"
-- >   addSystem app "rise" Update (write transform) $ \sys _entity ref -> do
-- >     dt <- deltaSeconds sys
-- >     modifyRef ref $ \t -> t { translation = translation t + V3 0 dt 0 }
--
-- A system's query is built from 'readC', 'write', 'with' and 'without', combined
-- applicatively. The query decides both which entities the system visits and what the
-- system's function is handed for each one, so there are no pointers to cast and no way to
-- read a component the query didn't ask for.
--
-- The plugin is reloaded when its library is rebuilt. Component values and 'statePtr' blocks
-- survive; top-level 'IORef's and other Haskell values do not, because a reload is a fresh
-- copy of the program.
module Mira
  ( -- * The plugin
    App
  , plugin
    -- * Components
  , Component
  , registerComponent
  , lookupComponent
  , statePtr
  , FieldType (..)
  , Field (..)
  , describeComponent
    -- * Queries
  , Query
  , readC
  , write
  , with
  , without
  , Ref
  , readRef
  , writeRef
  , modifyRef
    -- * Systems
  , System
  , Stage (..)
  , Entity
  , addSystem
  , addSystem_
  , runBefore
  , runAfter
  , Each
  , addSystem1
  , addSystem2
  , addSystem3
  , forEach
  , fetch
  , deltaSeconds
  , elapsedSeconds
  , spawn
  , despawn
  , despawnTree
  , setParent
  , insert
  , remove
    -- * Input
  , Key (..)
  , MouseButton (..)
  , keyDown
  , keyPressed
  , keyReleased
  , mouseDown
  , mousePressed
  , mouseMotion
    -- * Drawing
  , Mesh
  , Vertex (..)
  , Material (..)
  , material
  , meshCube
  , meshSphere
  , meshPlane
  , meshCreate
  , setMesh
  , setMaterial
  , Image
  , loadImage
  , setTextures
  , spawnModel
  , spawnPrefab
    -- * Sound
  , Sound
  , Voice
  , soundTone
  , loadSound
  , playSound
  , playSoundWith
  , stopSound
  , soundPlaying
  , setSoundSource
  , setListener
  , setMasterVolume
    -- * Voxels
  , require
  , Block
  , air
  , VoxelHit (..)
  , voxelBlock
  , voxel
  , setVoxel
  , voxelSolid
  , voxelRaycast
  , setChunkViewer
    -- * The window and the game
  , setCursorGrabbed
  , cursorGrabbed
  , quit
    -- * Settings
  , setting
  , ViewMode (..)
  , setViewMode
    -- * Input actions
  , bindAction
  , bindAxis
  , ActionState (..)
  , actionState
  , actionValue
  , pointerOnGround
    -- * Pictures and sounds made from code
  , createImage
  , pixelated
  , linearImage
  , setAlphaCutoff
  , createSound
    -- * The game's own files
  , writeGameFile
  , readGameFile
    -- * Signals
  , SignalOp (..)
  , setSignal
  , setSignalNumber
  , signal
  , signalIsTrue
  , defineSignal
    -- ** Rules as expressions
  , Sig
  , sig
  , truth
  , number
  , notS
  , (.&&.)
  , (.||.)
  , (.<.)
  , (.<=.)
  , (.==.)
  , (.>=.)
  , (.>.)
  , countS
  , selectS
  , heldFor
  , timer
  , timerReset
  , defineRule
    -- * Events
  , Event
  , registerEvent
  , sendEvent
  , readEvents
    -- * The scene
  , Camera (..)
  , defaultCamera
  , Light (..)
  , defaultLight
  , setCamera
  , setCameraOrthographic
  , setLight
  , setAmbient
  , setWindowTitle
    -- * Physics
  , Shape (..)
  , Collider (..)
  , colliderOf
  , BodyKind (..)
  , Body (..)
  , bodyOf
  , setCollider
  , setBody
  , applyImpulse
  , setVelocity
  , getVelocity
  , RayHit (..)
  , raycast
  , Contact (..)
    -- * Logging
  , logInfo
  , logWarn
  , logError
    -- * Engine components
  , Transform (..)
  , lookingAt
  , V3 (..)
  , Quat (..)
  ) where

import Control.Exception (SomeException, displayException, try)
import Control.Monad (unless, when)
import Data.IORef (IORef, atomicModifyIORef', newIORef)
import Data.Int (Int32, Int64)
import Data.Word (Word32, Word64, Word8)
import Foreign
  ( Ptr
  , Storable (..)
  , alloca
  , allocaArray
  , castPtr
  , peekArray
  , peekElemOff
  , plusPtr
  , pokeElemOff
  , withArrayLen
  )
import qualified Foreign
import Foreign.C.String (withCStringLen)
import Foreign.C.Types (CChar, CDouble (..), CFloat (..), CInt (..), CSize (..))
import Foreign.StablePtr (StablePtr, castPtrToStablePtr, castStablePtrToPtr, deRefStablePtr, freeStablePtr, newStablePtr)
import System.IO.Unsafe (unsafePerformIO)

-- The engine's functions, by way of cbits/mira_hs.c. The ones called once per entity are
-- imported `unsafe`, which makes them as cheap as a C call; none of them calls back.

foreign import ccall unsafe "mira_hs_log" c_log :: Word32 -> Ptr CChar -> CSize -> IO ()
foreign import ccall unsafe "mira_hs_component_register"
  c_component_register :: Ptr () -> Ptr CChar -> CSize -> CSize -> CSize -> IO Word32
foreign import ccall unsafe "mira_hs_component_lookup"
  c_component_lookup :: Ptr () -> Ptr CChar -> CSize -> Ptr CSize -> Ptr CSize -> IO Word32
foreign import ccall unsafe "mira_hs_state"
  c_state :: Ptr () -> Ptr CChar -> CSize -> CSize -> CSize -> IO (Ptr ())
foreign import ccall unsafe "mira_hs_delta_seconds" c_delta_seconds :: Ptr () -> IO CFloat
foreign import ccall unsafe "mira_hs_elapsed_seconds" c_elapsed_seconds :: Ptr () -> IO CDouble
foreign import ccall unsafe "mira_hs_system_add_queries"
  c_system_add_queries ::
    Ptr () -> Ptr CChar -> CSize -> Word32 -> Ptr () -> Ptr Word32 -> Ptr CSize -> CSize -> IO CInt
foreign import ccall unsafe "mira_hs_query_next_in"
  c_query_next_in :: Ptr () -> Word32 -> Ptr Word64 -> Ptr (Ptr ()) -> IO Word8
foreign import ccall unsafe "mira_hs_query_get_in"
  c_query_get_in :: Ptr () -> Word32 -> Word64 -> Ptr (Ptr ()) -> IO Word8
foreign import ccall unsafe "mira_hs_query_rewind" c_query_rewind :: Ptr () -> Word32 -> IO ()
foreign import ccall unsafe "mira_hs_key_down" c_key_down :: Ptr () -> Word32 -> IO Word8
foreign import ccall unsafe "mira_hs_key_pressed" c_key_pressed :: Ptr () -> Word32 -> IO Word8
foreign import ccall unsafe "mira_hs_key_released" c_key_released :: Ptr () -> Word32 -> IO Word8
foreign import ccall unsafe "mira_hs_mouse_down" c_mouse_down :: Ptr () -> Word32 -> IO Word8
foreign import ccall unsafe "mira_hs_mouse_pressed" c_mouse_pressed :: Ptr () -> Word32 -> IO Word8
foreign import ccall unsafe "mira_hs_mouse_motion" c_mouse_motion :: Ptr () -> Ptr CFloat -> IO ()
foreign import ccall unsafe "mira_hs_mesh_shape" c_mesh_shape :: Ptr () -> Word32 -> CFloat -> IO Word32
foreign import ccall unsafe "mira_hs_mesh_create"
  c_mesh_create :: Ptr () -> Ptr Vertex -> CSize -> Ptr Word32 -> CSize -> IO Word32
foreign import ccall unsafe "mira_hs_set_mesh" c_set_mesh :: Ptr () -> Word64 -> Word32 -> IO ()
foreign import ccall unsafe "mira_hs_set_material" c_set_material :: Ptr () -> Word64 -> Ptr Material -> IO ()
foreign import ccall unsafe "mira_hs_event_register"
  c_event_register :: Ptr () -> Ptr CChar -> CSize -> CSize -> IO Word32
foreign import ccall unsafe "mira_hs_event_send" c_event_send :: Ptr () -> Word32 -> Ptr () -> IO ()
foreign import ccall unsafe "mira_hs_event_next" c_event_next :: Ptr () -> Word32 -> Ptr () -> IO Word8
foreign import ccall unsafe "mira_hs_set_camera_orthographic"
  c_set_camera_orthographic :: Ptr () -> Word64 -> CFloat -> CFloat -> CFloat -> IO ()
foreign import ccall unsafe "mira_hs_system_order"
  c_system_order :: Ptr () -> Ptr CChar -> CSize -> Word32 -> Ptr CChar -> CSize -> IO CInt
foreign import ccall unsafe "mira_hs_set_camera"
  c_set_camera :: Ptr () -> Word64 -> CFloat -> CFloat -> Word32 -> IO ()
foreign import ccall unsafe "mira_hs_set_light"
  c_set_light :: Ptr () -> Word64 -> CFloat -> CFloat -> CFloat -> CFloat -> Word32 -> IO ()
foreign import ccall unsafe "mira_hs_set_ambient"
  c_set_ambient :: Ptr () -> CFloat -> CFloat -> CFloat -> CFloat -> IO ()
foreign import ccall unsafe "mira_hs_set_window_title"
  c_set_window_title :: Ptr () -> Ptr CChar -> CSize -> IO ()
foreign import ccall unsafe "mira_hs_set_collider"
  c_set_collider ::
    Ptr () -> Word64 -> Word32 -> CFloat -> CFloat -> CFloat -> CFloat -> CFloat -> Word32 -> IO ()
foreign import ccall unsafe "mira_hs_set_body"
  c_set_body :: Ptr () -> Word64 -> Word32 -> CFloat -> CFloat -> CFloat -> CFloat -> Word32 -> IO ()
foreign import ccall unsafe "mira_hs_apply_impulse"
  c_apply_impulse :: Ptr () -> Word64 -> CFloat -> CFloat -> CFloat -> IO ()
foreign import ccall unsafe "mira_hs_set_velocity"
  c_set_velocity :: Ptr () -> Word64 -> CFloat -> CFloat -> CFloat -> IO ()
foreign import ccall unsafe "mira_hs_velocity" c_velocity :: Ptr () -> Word64 -> Ptr CFloat -> IO Word8
foreign import ccall unsafe "mira_hs_raycast"
  c_raycast ::
    Ptr () -> CFloat -> CFloat -> CFloat -> CFloat -> CFloat -> CFloat -> CFloat -> Ptr Word64 ->
    Ptr CFloat -> IO Word8
foreign import ccall unsafe "mira_hs_component_describe"
  c_component_describe ::
    Ptr () -> Word32 -> Ptr CChar -> Ptr CSize -> Ptr Word32 -> Ptr Word32 -> Ptr CSize -> CSize ->
    IO CInt
foreign import ccall unsafe "mira_hs_image_load" c_image_load :: Ptr () -> Ptr CChar -> CSize -> IO Word32
foreign import ccall unsafe "mira_hs_set_textures"
  c_set_textures :: Ptr () -> Word64 -> Word32 -> Word32 -> Word32 -> IO ()
foreign import ccall unsafe "mira_hs_spawn_model"
  c_spawn_model :: Ptr () -> Ptr CChar -> CSize -> Ptr Transform -> IO Word64
foreign import ccall unsafe "mira_hs_signal_set"
  c_signal_set :: Ptr () -> Ptr CChar -> CSize -> CDouble -> Word32 -> IO ()
foreign import ccall unsafe "mira_hs_signal_get"
  c_signal_get :: Ptr () -> Ptr CChar -> CSize -> Ptr CDouble -> IO Word32
foreign import ccall unsafe "mira_hs_signal_define"
  c_signal_define :: Ptr () -> Ptr CChar -> CSize -> Word32 -> CDouble -> Ptr CChar -> CSize -> IO ()
foreign import ccall unsafe "mira_hs_system_fail"
  c_system_fail :: Ptr () -> Ptr CChar -> CSize -> IO ()
foreign import ccall unsafe "mira_hs_spawn_prefab"
  c_spawn_prefab :: Ptr () -> Ptr CChar -> CSize -> Ptr Transform -> IO Word64
foreign import ccall unsafe "mira_hs_set_parent" c_set_parent :: Ptr () -> Word64 -> Word64 -> IO ()
foreign import ccall unsafe "mira_hs_despawn_tree" c_despawn_tree :: Ptr () -> Word64 -> IO ()
foreign import ccall unsafe "mira_hs_spawn" c_spawn :: Ptr () -> IO Word64
foreign import ccall unsafe "mira_hs_despawn" c_despawn :: Ptr () -> Word64 -> IO ()
foreign import ccall unsafe "mira_hs_insert" c_insert :: Ptr () -> Word64 -> Word32 -> Ptr () -> IO ()
foreign import ccall unsafe "mira_hs_remove" c_remove :: Ptr () -> Word64 -> Word32 -> IO ()

-- | The app being set up. Only meaningful inside the function given to 'plugin'.
newtype App = App (Ptr ())

-- | One run of a system. Only meaningful inside that system's function.
newtype System = System (Ptr ())

-- | An entity. Valid until it is despawned.
newtype Entity = Entity Word64
  deriving (Eq, Ord, Show)

-- | A component whose values are of type @a@.
newtype Component a = Component Word32

-- | When in the frame a system runs.
data Stage
  = -- | Once, the first time the plugin is loaded (not again on reload).
    Startup
  | First
  | PreUpdate
  | -- | Fixed timestep: zero or more times a frame.
    FixedUpdate
  | Update
  | PostUpdate
  | Last
  deriving (Eq, Show, Enum)

logAt :: Word32 -> String -> IO ()
logAt level message =
  withCStringLen message $ \(chars, len) -> c_log level chars (fromIntegral len)

logError, logWarn, logInfo :: String -> IO ()
logError = logAt 1
logWarn = logAt 2
logInfo = logAt 3

-- | Wraps a plugin's setup function as its entry point. An exception thrown while setting up
-- is logged and makes the load fail, which leaves the previous version (if any) unloaded and
-- the engine running.
plugin :: (App -> IO ()) -> Ptr () -> IO CInt
plugin setup app = do
  result <- try (setup (App app))
  case result of
    Right () -> pure 0
    Left (err :: SomeException) -> do
      logError ("plugin failed to load: " ++ displayException err)
      pure (-1)

withName :: String -> (Ptr CChar -> CSize -> IO a) -> IO a
withName name use = withCStringLen name $ \(chars, len) -> use chars (fromIntegral len)

-- | Defines a component stored as the bytes of @a@, or finds it if this plugin defined it
-- before a reload. Its values are kept across reloads as long as @a@ keeps its size and
-- alignment.
registerComponent :: forall a. Storable a => App -> String -> IO (Component a)
registerComponent (App app) name = do
  let proxy = undefined :: a
  handle <- withName name $ \chars len ->
    c_component_register app chars len (fromIntegral (sizeOf proxy)) (fromIntegral (alignment proxy))
  when (handle == 0) $ ioError (userError ("could not register component `" ++ name ++ "`"))
  pure (Component handle)

-- | Finds a component defined by the engine or another plugin. Fails if there is none, or if
-- @a@ is not the size and alignment the engine has for it.
lookupComponent :: forall a. Storable a => App -> String -> IO (Component a)
lookupComponent (App app) name = do
  let proxy = undefined :: a
  (handle, size, align) <- withName name $ \chars len ->
    alloca $ \sizeOut -> alloca $ \alignOut -> do
      handle <- c_component_lookup app chars len sizeOut alignOut
      (,,) handle <$> peek sizeOut <*> peek alignOut
  when (handle == 0) $ ioError (userError ("no component named `" ++ name ++ "`"))
  unless (fromIntegral size == sizeOf proxy && fromIntegral align == alignment proxy) $
    ioError . userError $
      "component `" ++ name ++ "` is " ++ show size ++ " bytes aligned to " ++ show align
        ++ ", but the plugin's type is " ++ show (sizeOf proxy) ++ " bytes aligned to "
        ++ show (alignment proxy)
  pure (Component handle)

-- | A block of memory owned by the engine, zeroed when first asked for, that survives
-- reloads. This is where a plugin keeps anything a top-level 'IORef' would otherwise hold.
statePtr :: forall a. Storable a => App -> String -> IO (Ptr a)
statePtr (App app) name = do
  let proxy = undefined :: a
  castPtr <$> withName name (\chars len ->
    c_state app chars len (fromIntegral (sizeOf proxy)) (fromIntegral (alignment proxy)))

-- | What one field of a component holds.
data FieldType
  = FieldF32
  | FieldF64
  | FieldI32
  | FieldI64
  | FieldU8
  | FieldU32
  | -- | One byte; zero is false.
    FieldBool
  | -- | An 'Entity'. Scenes keep it pointing at the right entity.
    FieldEntity
  deriving (Eq, Show, Enum)

-- | One field of a component: its name, what it holds, how many in a row (more than one for
-- a vector or an array), and how many bytes into the component it starts.
data Field = Field String FieldType Int Int
  deriving (Eq, Show)

-- | Says what fields a component this plugin registered has, as its 'Storable' instance lays
-- them out. From then on the engine can save the component in scenes, load it back, and show
-- it in an inspector. Bytes that no field covers aren't saved, and load as zero.
describeComponent :: App -> Component a -> [Field] -> IO ()
describeComponent (App app) (Component handle) fields =
  withCStringLen (concat [name | Field name _ _ _ <- fields]) $ \(names, _) -> do
    -- The length of each name in bytes, which is what the engine counts in.
    lens <- mapM (\(Field name _ _ _) -> withCStringLen name (pure . fromIntegral . snd)) fields
    status <-
      withArrayLen (lens :: [CSize]) $ \count lensPtr ->
        withArrayLen [fromIntegral (fromEnum kind) | Field _ kind _ _ <- fields] $ \_ types ->
          withArrayLen [fromIntegral n | Field _ _ n _ <- fields] $ \_ counts ->
            withArrayLen [fromIntegral offset | Field _ _ _ offset <- fields] $ \_ offsets ->
              c_component_describe app handle names lensPtr types counts offsets (fromIntegral count)
    when (status /= 0) $
      ioError (userError "could not describe the component (see the engine's log)")

-- | What a system asks of each entity it visits, and so what its function receives.
--
-- > (,) <$> write transform <*> readC velocity <* without frozen
data Query a = Query
  { queryTerms :: [(Word32, Word32)]
    -- ^ component handle and access mode, in order
  , querySlots :: Int
    -- ^ how many pointers the engine hands back per entity
  , queryDecode :: Ptr (Ptr ()) -> Int -> IO a
    -- ^ builds the result from those pointers, starting at the given slot
  }

instance Functor Query where
  fmap f (Query terms slots decode) = Query terms slots (\found at -> f <$> decode found at)

instance Applicative Query where
  pure x = Query [] 0 (\_ _ -> pure x)
  Query termsF slotsF decodeF <*> Query termsX slotsX decodeX =
    Query
      (termsF ++ termsX)
      (slotsF + slotsX)
      (\found at -> decodeF found at <*> decodeX found (at + slotsF))

-- | Visit entities that have the component, and receive its value.
readC :: Storable a => Component a -> Query a
readC (Component handle) =
  Query [(handle, 0)] 1 (\found at -> peekElemOff found at >>= peek . castPtr)

-- | A component of the entity being visited, which the system may change.
newtype Ref a = Ref (Ptr a)

-- | Visit entities that have the component, and receive a reference to change it through.
write :: Component a -> Query (Ref a)
write (Component handle) =
  Query [(handle, 1)] 1 (\found at -> Ref . castPtr <$> peekElemOff found at)

-- | Only visit entities that have the component.
with :: Component a -> Query ()
with (Component handle) = Query [(handle, 2)] 0 (\_ _ -> pure ())

-- | Only visit entities that don't have the component.
without :: Component a -> Query ()
without (Component handle) = Query [(handle, 3)] 0 (\_ _ -> pure ())

readRef :: Storable a => Ref a -> IO a
readRef (Ref ptr) = peek ptr

writeRef :: Storable a => Ref a -> a -> IO ()
writeRef (Ref ptr) = poke ptr

modifyRef :: Storable a => Ref a -> (a -> a) -> IO ()
modifyRef ref f = readRef ref >>= writeRef ref . f

-- Every system's Haskell function, kept alive for the engine to call. They are released when
-- this version of the plugin is unloaded, so the collector can let go of what they captured.
{-# NOINLINE systems #-}
systems :: IORef [StablePtr (Ptr () -> IO ())]
systems = unsafePerformIO (newIORef [])

-- | One of a system's queries, ready to be walked while the system runs.
data Each a = Each (Ptr ()) Word32 (Query a)

-- | Calls the function for every entity the query matches. Calling it again starts over.
-- (Walking the same 'Each' inside its own 'forEach' restarts the outer walk too; give the
-- system two queries instead.)
forEach :: Each a -> (Entity -> a -> IO ()) -> IO ()
forEach (Each system index query) visit = do
  c_query_rewind system index
  allocaArray (max 1 (querySlots query)) $ \found -> alloca $ \entityOut ->
    let loop = do
          more <- c_query_next_in system index entityOut found
          when (more /= 0) $ do
            entity <- peek entityOut
            item <- queryDecode query found 0
            visit (Entity entity) item
            loop
     in loop

-- | Looks one entity up. 'Nothing' if the query doesn't match it.
fetch :: Each a -> Entity -> IO (Maybe a)
fetch (Each system index query) (Entity entity) =
  allocaArray (max 1 (querySlots query)) $ \found -> do
    matched <- c_query_get_in system index entity found
    if matched /= 0 then Just <$> queryDecode query found 0 else pure Nothing

-- Registers a system whose queries have these term lists, to run `body`.
register :: App -> String -> Stage -> [[(Word32, Word32)]] -> (Ptr () -> IO ()) -> IO ()
register (App app) name stage termLists body = do
  closure <- newStablePtr body
  atomicModifyIORef' systems (\held -> (closure : held, ()))
  let flat = concat [[handle, access] | terms <- termLists, (handle, access) <- terms]
      counts = map (fromIntegral . length) termLists :: [CSize]
  status <- withName name $ \chars len ->
    withArrayLen flat $ \_ terms -> withArrayLen counts $ \queries countsPtr ->
      c_system_add_queries app chars len (fromIntegral (fromEnum stage)) (castStablePtrToPtr closure)
        terms countsPtr (fromIntegral queries)
  when (status /= 0) $
    ioError . userError $
      "could not add system `" ++ name ++ "` (see the engine's log; two queries of one system "
        ++ "may not reach the same component unless neither writes it or `with`/`without` keep "
        ++ "them apart)"

-- | Adds a system that visits every entity matching the query, calling the function once for
-- each with the entity and what the query asked for.
addSystem :: App -> String -> Stage -> Query a -> (System -> Entity -> a -> IO ()) -> IO ()
addSystem app name stage query visit =
  addSystem1 app name stage query $ \sys each -> forEach each (visit sys)

-- | Says that the system added under the first name runs before the other, within their
-- stage. The other is another of this plugin's systems by the name it was added under, or
-- any system or set by its full name (@"otherplugin::system"@, @"signals"@).
runBefore :: App -> String -> String -> IO ()
runBefore = order 0

-- | Says that the system added under the first name runs after the other. See 'runBefore'.
runAfter :: App -> String -> String -> IO ()
runAfter = order 1

order :: Word32 -> App -> String -> String -> IO ()
order relation (App app) name other =
  withName name $ \chars len -> withName other $ \otherChars otherLen -> do
    status <- c_system_order app chars len relation otherChars otherLen
    when (status /= 0) $ ioError (userError ("there is no system `" ++ name ++ "` to order"))

-- | Adds a system that runs once each time its stage comes round, visiting nothing.
addSystem_ :: App -> String -> Stage -> (System -> IO ()) -> IO ()
addSystem_ app name stage body = register app name stage [[]] (body . System)

-- | Adds a system that runs once each time its stage comes round, with a query to walk
-- ('forEach') or look entities up in ('fetch') as it chooses.
addSystem1 :: App -> String -> Stage -> Query a -> (System -> Each a -> IO ()) -> IO ()
addSystem1 app name stage qa body =
  register app name stage [queryTerms qa] $ \sys -> body (System sys) (Each sys 0 qa)

-- | As 'addSystem1', with two queries: say, the player and the things it can pick up. The
-- engine refuses the system if both queries could reach the same component of the same
-- entity and either writes it; 'with' and 'without' are how to keep them apart.
addSystem2 ::
  App -> String -> Stage -> Query a -> Query b -> (System -> Each a -> Each b -> IO ()) -> IO ()
addSystem2 app name stage qa qb body =
  register app name stage [queryTerms qa, queryTerms qb] $ \sys ->
    body (System sys) (Each sys 0 qa) (Each sys 1 qb)

addSystem3 ::
  App ->
  String ->
  Stage ->
  Query a ->
  Query b ->
  Query c ->
  (System -> Each a -> Each b -> Each c -> IO ()) ->
  IO ()
addSystem3 app name stage qa qb qc body =
  register app name stage [queryTerms qa, queryTerms qb, queryTerms qc] $ \sys ->
    body (System sys) (Each sys 0 qa) (Each sys 1 qb) (Each sys 2 qc)

-- | Seconds since the last frame (or the fixed timestep, in 'FixedUpdate').
deltaSeconds :: System -> IO Float
deltaSeconds (System system) = realToFrac <$> c_delta_seconds system

-- | Seconds since the app started.
elapsedSeconds :: System -> IO Double
elapsedSeconds (System system) = realToFrac <$> c_elapsed_seconds system

-- | Creates an entity. It can be given components at once; it shows up in queries after this
-- system returns.
spawn :: System -> IO Entity
spawn (System system) = Entity <$> c_spawn system

despawn :: System -> Entity -> IO ()
despawn (System system) (Entity entity) = c_despawn system entity

-- | Despawns an entity and everything below it (a model and its parts, say), when this
-- system returns.
despawnTree :: System -> Entity -> IO ()
despawnTree (System system) (Entity entity) = c_despawn_tree system entity

-- | Makes the first entity a child of the second, so that its transform is relative to the
-- parent's, or a root again with 'Nothing'. Takes effect when this system returns.
setParent :: System -> Entity -> Maybe Entity -> IO ()
setParent (System system) (Entity child) parent =
  c_set_parent system child (maybe maxBound (\(Entity bits) -> bits) parent)

-- | Adds or replaces a component when this system returns.
insert :: Storable a => System -> Entity -> Component a -> a -> IO ()
insert (System system) (Entity entity) (Component handle) value =
  Foreign.with value $ \ptr -> c_insert system entity handle (castPtr ptr)

remove :: System -> Entity -> Component a -> IO ()
remove (System system) (Entity entity) (Component handle) = c_remove system entity handle

-- | A key, by its position on a US keyboard (so 'KeyW' is the same key on every layout).
data Key
  = KeyA | KeyB | KeyC | KeyD | KeyE | KeyF | KeyG | KeyH | KeyI | KeyJ | KeyK | KeyL | KeyM
  | KeyN | KeyO | KeyP | KeyQ | KeyR | KeyS | KeyT | KeyU | KeyV | KeyW | KeyX | KeyY | KeyZ
  | Digit0 | Digit1 | Digit2 | Digit3 | Digit4 | Digit5 | Digit6 | Digit7 | Digit8 | Digit9
  | Space | Enter | Escape | Tab | Backspace
  | ArrowLeft | ArrowRight | ArrowUp | ArrowDown
  | ShiftLeft | ShiftRight | ControlLeft | ControlRight | AltLeft | AltRight
  | F1 | F2 | F3 | F4 | F5 | F6 | F7 | F8 | F9 | F10 | F11 | F12
  deriving (Eq, Show, Enum, Bounded)

data MouseButton = MouseLeft | MouseRight | MouseMiddle
  deriving (Eq, Show, Enum, Bounded)

asked :: (Ptr () -> Word32 -> IO Word8) -> Enum k => System -> k -> IO Bool
asked ask (System system) k = (/= 0) <$> ask system (fromIntegral (fromEnum k))

-- | Whether the key is held down.
keyDown :: System -> Key -> IO Bool
keyDown = asked c_key_down

-- | Whether the key went down this frame.
keyPressed :: System -> Key -> IO Bool
keyPressed = asked c_key_pressed

-- | Whether the key came up this frame.
keyReleased :: System -> Key -> IO Bool
keyReleased = asked c_key_released

mouseDown :: System -> MouseButton -> IO Bool
mouseDown = asked c_mouse_down

mousePressed :: System -> MouseButton -> IO Bool
mousePressed = asked c_mouse_pressed

-- | How far the mouse moved this frame, in pixels: x, then y.
mouseMotion :: System -> IO (Float, Float)
mouseMotion (System system) = allocaArray 2 $ \delta -> do
  c_mouse_motion system delta
  (,) . realToFrac <$> peekElemOff delta 0 <*> (realToFrac <$> peekElemOff delta 1)

-- | A mesh, shared between the entities drawn with it. Make it once, in a 'Startup' system,
-- and keep it in a 'statePtr' block: it is 'Storable', and still good after a reload.
newtype Mesh = Mesh Word32
  deriving (Eq, Show)

instance Storable Mesh where
  sizeOf _ = 4
  alignment _ = 4
  peek ptr = Mesh <$> peek (castPtr ptr)
  poke ptr (Mesh handle) = poke (castPtr ptr) handle

-- | One corner of a mesh: position, normal and texture coordinates.
data Vertex = Vertex !V3 !V3 !Float !Float

instance Storable Vertex where
  sizeOf _ = 32
  alignment _ = 4
  peek ptr =
    Vertex <$> peek (castPtr ptr) <*> peek (castPtr (ptr `plusPtr` 12))
      <*> peek (castPtr (ptr `plusPtr` 24)) <*> peek (castPtr (ptr `plusPtr` 28))
  poke ptr (Vertex position normal u v) = do
    poke (castPtr ptr) position
    poke (castPtr (ptr `plusPtr` 12)) normal
    poke (castPtr (ptr `plusPtr` 24)) u
    poke (castPtr (ptr `plusPtr` 28)) v

-- | How a surface looks. Colours are linear red, green and blue.
data Material = Material
  { color :: !V3
  , opacity :: !Float
  , emissive :: !V3
    -- ^ light the surface gives off itself
  , roughness :: !Float
    -- ^ 0 mirror-smooth to 1 matte
  , metallic :: !Float
  }
  deriving (Eq, Show)

-- | A plain, matte surface of the given colour.
material :: V3 -> Material
material rgb = Material rgb 1 0 0.8 0

instance Storable Material where
  sizeOf _ = 36
  alignment _ = 4
  peek ptr =
    Material <$> peek (castPtr ptr) <*> peek (castPtr (ptr `plusPtr` 12))
      <*> peek (castPtr (ptr `plusPtr` 16)) <*> peek (castPtr (ptr `plusPtr` 28))
      <*> peek (castPtr (ptr `plusPtr` 32))
  poke ptr (Material rgb alpha glow rough metal) = do
    poke (castPtr ptr) rgb
    poke (castPtr (ptr `plusPtr` 12)) alpha
    poke (castPtr (ptr `plusPtr` 16)) glow
    poke (castPtr (ptr `plusPtr` 28)) rough
    poke (castPtr (ptr `plusPtr` 32)) metal

made :: String -> Word32 -> IO Mesh
made what handle = do
  when (handle == 0) $ ioError (userError ("could not make " ++ what ++ " (does the app have a renderer?)"))
  pure (Mesh handle)

-- | A cube with the given edge length.
meshCube :: System -> Float -> IO Mesh
meshCube (System system) size = c_mesh_shape system 0 (realToFrac size) >>= made "a cube mesh"

meshSphere :: System -> Float -> IO Mesh
meshSphere (System system) radius = c_mesh_shape system 1 (realToFrac radius) >>= made "a sphere mesh"

-- | A flat square facing up, with the given edge length.
meshPlane :: System -> Float -> IO Mesh
meshPlane (System system) size = c_mesh_shape system 2 (realToFrac size) >>= made "a plane mesh"

-- | A mesh from triangles: three indices each, counter-clockwise seen from the front.
meshCreate :: System -> [Vertex] -> [Word32] -> IO Mesh
meshCreate (System system) vertices indices =
  withArrayLen vertices $ \vertexCount vertexPtr -> withArrayLen indices $ \indexCount indexPtr ->
    c_mesh_create system vertexPtr (fromIntegral vertexCount) indexPtr (fromIntegral indexCount)
      >>= made "a mesh"

-- | Draws the entity as the mesh (it also needs a 'Transform'), when this system returns.
setMesh :: System -> Entity -> Mesh -> IO ()
setMesh (System system) (Entity entity) (Mesh handle) = c_set_mesh system entity handle

-- | Sets the entity's surface, when this system returns.
setMaterial :: System -> Entity -> Material -> IO ()
setMaterial (System system) (Entity entity) surface =
  Foreign.with surface (c_set_material system entity)

-- | An event type whose events are values of @a@.
newtype Event a = Event Word32

-- | Defines an event type, or finds the one the name already has. Any plugin that knows the
-- name can send and read it, in any language; that is how plugins talk to each other. The
-- engine's own events are found the same way: @"mira.Contact"@ is a 'Contact'.
registerEvent :: forall a. Storable a => App -> String -> IO (Event a)
registerEvent (App app) name = do
  handle <- withName name $ \chars len ->
    c_event_register app chars len (fromIntegral (sizeOf (undefined :: a)))
  when (handle == 0) $
    ioError (userError ("could not register event `" ++ name ++ "` (is it another size?)"))
  pure (Event handle)

-- | Sends an event when this system returns. Systems later this frame, and every system next
-- frame, can read it.
sendEvent :: Storable a => System -> Event a -> a -> IO ()
sendEvent (System system) (Event handle) value =
  Foreign.with value $ \ptr -> c_event_send system handle (castPtr ptr)

-- | The events this system hasn't read yet, oldest first. Each system reads each event once,
-- and a reloaded system carries on where the old one stopped.
readEvents :: Storable a => System -> Event a -> IO [a]
readEvents (System system) (Event handle) = alloca $ \ptr ->
  let loop seen = do
        more <- c_event_next system handle (castPtr ptr)
        if more /= 0 then peek ptr >>= \event -> loop (event : seen) else pure (reverse seen)
   in loop []

flag :: Bool -> Word32
flag = fromIntegral . fromEnum

-- | What the scene is seen through. The first active camera is the one drawn.
data Camera = Camera
  { fieldOfView :: !Float
    -- ^ vertical, in radians
  , nearPlane :: !Float
  , cameraActive :: !Bool
  }
  deriving (Eq, Show)

-- | A camera with a 60 degree field of view.
defaultCamera :: Camera
defaultCamera = Camera (pi / 3) 0.1 True

-- | A sun-like light, shining along the forward direction (-Z) of the entity's transform.
data Light = Light
  { lightColor :: !V3
  , lightIntensity :: !Float
  , castsShadows :: !Bool
  }
  deriving (Eq, Show)

-- | White sunlight that casts shadows.
defaultLight :: Light
defaultLight = Light (V3 1 1 1) 3 True

-- | Makes the entity a camera (it also needs a 'Transform'), when this system returns.
setCamera :: System -> Entity -> Camera -> IO ()
setCamera (System system) (Entity entity) (Camera fov near on) =
  c_set_camera system entity (realToFrac fov) (realToFrac near) (flag on)

-- | Makes the entity an orthographic camera (it also needs a 'Transform'), when this system
-- returns: the view is that many metres of the world tall, and things don't shrink with
-- distance. The other two numbers are the nearest and farthest distances drawn.
setCameraOrthographic :: System -> Entity -> Float -> Float -> Float -> IO ()
setCameraOrthographic (System system) (Entity entity) height near far =
  c_set_camera_orthographic system entity (realToFrac height) (realToFrac near) (realToFrac far)

-- | Makes the entity a light (it also needs a 'Transform'), when this system returns.
setLight :: System -> Entity -> Light -> IO ()
setLight (System system) (Entity entity) (Light (V3 r g b) strength cast) =
  c_set_light system entity (realToFrac r) (realToFrac g) (realToFrac b) (realToFrac strength) (flag cast)

-- | Sets the light arriving from every direction: a colour and a strength.
setAmbient :: System -> V3 -> Float -> IO ()
setAmbient (System system) (V3 r g b) strength =
  c_set_ambient system (realToFrac r) (realToFrac g) (realToFrac b) (realToFrac strength)

setWindowTitle :: System -> String -> IO ()
setWindowTitle (System system) title = withName title (c_set_window_title system)

-- | The solid form of a collider.
data Shape
  = Sphere !Float
  | -- | Half extents along x, y and z.
    Box !V3
  | -- | Upright: a radius and a height.
    Capsule !Float !Float
  | -- | Everything below the entity's position.
    Ground
  deriving (Eq, Show)

-- | Makes an entity solid. Without a 'Body' it never moves.
data Collider = Collider
  { colliderShape :: !Shape
  , friction :: !Float
  , restitution :: !Float
    -- ^ bounciness, 0 to 1
  , isSensor :: !Bool
    -- ^ detects overlaps but blocks nothing
  }
  deriving (Eq, Show)

-- | A collider of the given shape with ordinary friction and no bounce.
colliderOf :: Shape -> Collider
colliderOf form = Collider form 0.5 0 False

data BodyKind
  = -- | Moved by gravity, forces and collisions.
    Dynamic
  | -- | Moved only by its velocity; pushes other bodies and is never pushed.
    Kinematic
  | -- | Moved by setting its transform; pushes other bodies and is never pushed.
    Animated
  deriving (Eq, Show, Enum)

-- | Makes an entity with a collider move.
data Body = Body
  { bodyKind :: !BodyKind
  , bodyMass :: !Float
    -- ^ kilograms; 0 to work it out from the collider's size
  , bodyVelocity :: !V3
  , lockRotation :: !Bool
    -- ^ keeps it upright
  }
  deriving (Eq, Show)

-- | A body of the given kind, at rest, with its mass worked out from its collider.
bodyOf :: BodyKind -> Body
bodyOf how = Body how 0 (V3 0 0 0) False

-- | Makes the entity solid, when this system returns. Does nothing in an app without physics.
setCollider :: System -> Entity -> Collider -> IO ()
setCollider (System system) (Entity entity) (Collider form rough bounce ghost) =
  c_set_collider system entity code (realToFrac x) (realToFrac y) (realToFrac z)
    (realToFrac rough) (realToFrac bounce) (flag ghost)
  where
    (code, V3 x y z) = case form of
      Sphere radius -> (0, V3 radius 0 0)
      Box half -> (1, half)
      Capsule radius height -> (2, V3 radius height 0)
      Ground -> (3, V3 0 0 0)

-- | Makes an entity with a collider move, when this system returns.
setBody :: System -> Entity -> Body -> IO ()
setBody (System system) (Entity entity) (Body how kilograms (V3 x y z) upright) =
  c_set_body system entity (fromIntegral (fromEnum how)) (realToFrac kilograms)
    (realToFrac x) (realToFrac y) (realToFrac z) (flag upright)

-- | A sudden push on a dynamic body, in newton-seconds.
applyImpulse :: System -> Entity -> V3 -> IO ()
applyImpulse (System system) (Entity entity) (V3 x y z) =
  c_apply_impulse system entity (realToFrac x) (realToFrac y) (realToFrac z)

setVelocity :: System -> Entity -> V3 -> IO ()
setVelocity (System system) (Entity entity) (V3 x y z) =
  c_set_velocity system entity (realToFrac x) (realToFrac y) (realToFrac z)

peekV3 :: Ptr CFloat -> Int -> IO V3
peekV3 floats at =
  V3 . realToFrac <$> peekElemOff floats at
    <*> (realToFrac <$> peekElemOff floats (at + 1))
    <*> (realToFrac <$> peekElemOff floats (at + 2))

-- | The body's velocity, or 'Nothing' if the entity has no body.
getVelocity :: System -> Entity -> IO (Maybe V3)
getVelocity (System system) (Entity entity) = allocaArray 3 $ \floats -> do
  found <- c_velocity system entity floats
  if found /= 0 then Just <$> peekV3 floats 0 else pure Nothing

-- | What a ray met.
data RayHit = RayHit
  { hitEntity :: !Entity
  , hitPoint :: !V3
  , hitNormal :: !V3
  , hitDistance :: !Float
  }
  deriving (Eq, Show)

-- | The first collider a ray from the origin along the direction meets within the distance,
-- as things stood after the last physics step.
raycast :: System -> V3 -> V3 -> Float -> IO (Maybe RayHit)
raycast (System system) (V3 ox oy oz) (V3 dx dy dz) reach =
  alloca $ \entityOut -> allocaArray 7 $ \floats -> do
    found <-
      c_raycast system (realToFrac ox) (realToFrac oy) (realToFrac oz) (realToFrac dx)
        (realToFrac dy) (realToFrac dz) (realToFrac reach) entityOut floats
    if found == 0
      then pure Nothing
      else do
        entity <- peek entityOut
        hit <- RayHit (Entity entity) <$> peekV3 floats 0 <*> peekV3 floats 3
        Just . hit . realToFrac <$> peekElemOff floats 6

-- | The engine's @"mira.Contact"@ event: two colliders touched during a physics step.
data Contact = Contact
  { contactA :: !Entity
  , contactB :: !Entity
  , contactPoint :: !V3
  , contactNormal :: !V3
    -- ^ from the first toward the second
  , contactImpulse :: !Float
    -- ^ how hard, in newton-seconds
  }
  deriving (Eq, Show)

instance Storable Contact where
  sizeOf _ = 48
  alignment _ = 8
  peek ptr =
    Contact . Entity <$> peek (castPtr ptr) <*> (Entity <$> peek (castPtr (ptr `plusPtr` 8)))
      <*> peek (castPtr (ptr `plusPtr` 16)) <*> peek (castPtr (ptr `plusPtr` 28))
      <*> peek (castPtr (ptr `plusPtr` 40))
  poke ptr (Contact (Entity a) (Entity b) point normal push) = do
    poke (castPtr ptr) a
    poke (castPtr (ptr `plusPtr` 8)) b
    poke (castPtr (ptr `plusPtr` 16)) point
    poke (castPtr (ptr `plusPtr` 28)) normal
    poke (castPtr (ptr `plusPtr` 40)) push

-- | An image, shared between the materials that use it. 'Storable', so it can be kept in a
-- 'statePtr' block.
newtype Image = Image Word32
  deriving (Eq, Show)

instance Storable Image where
  sizeOf _ = 4
  alignment _ = 4
  peek ptr = Image <$> peek (castPtr ptr)
  poke ptr (Image handle) = poke (castPtr ptr) handle

-- | Loads an image by name: a PNG or JPEG path relative to the app's asset folder, with
-- @?linear@ appended for data such as normal maps. The same name always gives the same image.
-- It arrives a moment later; until then it draws as plain white.
loadImage :: System -> String -> IO Image
loadImage (System system) name = do
  handle <- withName name (c_image_load system)
  when (handle == 0) $ ioError (userError ("could not load `" ++ name ++ "` (no asset server?)"))
  pure (Image handle)

-- | Sets the textures of the entity's material, when this system returns: its colour, its
-- normal map, and its roughness and metalness.
setTextures :: System -> Entity -> Maybe Image -> Maybe Image -> Maybe Image -> IO ()
setTextures (System system) (Entity entity) baseColor normal metallicRoughness =
  c_set_textures system entity (raw baseColor) (raw normal) (raw metallicRoughness)
  where
    raw = maybe 0 (\(Image handle) -> handle)

-- | Spawns a glTF model (@.gltf@ or @.glb@) by name: a new entity at the transform, with a
-- child for each part of the model. The parts appear when this system returns.
spawnModel :: System -> String -> Transform -> IO Entity
spawnModel (System system) name at =
  withName name $ \chars len ->
    Foreign.with at (fmap Entity . c_spawn_model system chars len)

-- | Spawns an instance of a prefab by name (a scene file, or a name the app gave a scene):
-- a new entity at the transform, with the prefab's entities below it. They appear on the
-- next frame, and are rebuilt whenever the prefab's file is saved again.
spawnPrefab :: System -> String -> Transform -> IO Entity
spawnPrefab (System system) name at =
  withName name $ \chars len ->
    Foreign.with at (fmap Entity . c_spawn_prefab system chars len)

-- | How a signal is worked out from its inputs.
data SignalOp
  = SignalAnd
  | SignalOr
  | SignalNot
  | -- | How many inputs are true.
    SignalCount
  | SignalSum
  | -- | The second input if the first is true, otherwise the third.
    SignalSelect
  | -- | Seconds the first input has been true; a true second input resets it.
    SignalTimer
  | -- | True once the input has been true this many seconds without a break.
    SignalHeldFor !Double
  | SignalLess
  | SignalLessOrEqual
  | SignalEqual
  | SignalGreaterOrEqual
  | SignalGreater
  | -- | A setting: a number that stands until it is changed, in the graph or the rules' text.
    SignalNumber !Double
  | -- | A setting that is true or false.
    SignalTruth !Bool
  deriving (Eq, Show)

-- | Sets a true-or-false signal, defining it if need be, when this system returns: how a
-- plugin tells the signal graph a fact of its own.
setSignal :: System -> String -> Bool -> IO ()
setSignal (System system) name value =
  withName name $ \chars len -> c_signal_set system chars len (if value then 1 else 0) 0

-- | Sets a numeric signal, defining it if need be, when this system returns.
setSignalNumber :: System -> String -> Double -> IO ()
setSignalNumber (System system) name value =
  withName name $ \chars len -> c_signal_set system chars len (realToFrac value) 1

-- | A signal's value as of its last update, a truth being 1 or 0; 'Nothing' if there is no
-- such signal.
signal :: System -> String -> IO (Maybe Double)
signal (System system) name =
  withName name $ \chars len -> alloca $ \out -> do
    found <- c_signal_get system chars len out
    if found == 0 then pure Nothing else Just . realToFrac <$> peek out

-- | Whether a signal is true. One that doesn't exist is not.
signalIsTrue :: System -> String -> IO Bool
signalIsTrue system name = maybe False (/= 0) <$> signal system name

-- | Defines a signal worked out from others (a rule), or replaces its definition, when this
-- system returns. The inputs are signal names.
defineSignal :: System -> String -> SignalOp -> [String] -> IO ()
defineSignal (System system) name op inputs =
  withName name $ \chars len -> withName (unwords inputs) $ \inputChars inputLen ->
    c_signal_define system chars len code (realToFrac param) inputChars inputLen
  where
    (code, param) = case op of
      SignalAnd -> (0, 0 :: Double)
      SignalOr -> (1, 0)
      SignalNot -> (2, 0)
      SignalCount -> (3, 0)
      SignalSum -> (4, 0)
      SignalSelect -> (5, 0)
      SignalTimer -> (6, 0)
      SignalHeldFor seconds -> (7, seconds)
      SignalLess -> (8, 0)
      SignalLessOrEqual -> (9, 0)
      SignalEqual -> (10, 0)
      SignalGreaterOrEqual -> (11, 0)
      SignalGreater -> (12, 0)
      SignalNumber value -> (13, value)
      SignalTruth value -> (14, if value then 1 else 0)

-- | A signal as an expression over other signals: what a rule is.
--
-- > defineRule sys "red.clock" $
-- >   timer (sig "red.holds_all" .&&. notS (sig "blue.contesting"))
-- > defineRule sys "red.wins" (sig "red.clock" .>=. number 600)
--
-- Numbers add with '+' (and a literal is a constant), so @sig "a" + sig "b" .>. 3@ is a rule.
data Sig
  = SigNamed String
  | SigTruth Bool
  | SigNumber Double
  | SigOp SignalOp [Sig]

infixr 3 .&&.

infixr 2 .||.

infix 4 .<., .<=., .==., .>=., .>.

-- | Another signal, by name: one the host defines, another plugin sets, or another rule.
sig :: String -> Sig
sig = SigNamed

-- | A constant truth.
truth :: Bool -> Sig
truth = SigTruth

-- | A constant number.
number :: Double -> Sig
number = SigNumber

notS :: Sig -> Sig
notS a = SigOp SignalNot [a]

(.&&.), (.||.), (.<.), (.<=.), (.==.), (.>=.), (.>.) :: Sig -> Sig -> Sig
a .&&. b = SigOp SignalAnd (operands SignalAnd a ++ operands SignalAnd b)
a .||. b = SigOp SignalOr (operands SignalOr a ++ operands SignalOr b)
a .<. b = SigOp SignalLess [a, b]
a .<=. b = SigOp SignalLessOrEqual [a, b]
a .==. b = SigOp SignalEqual [a, b]
a .>=. b = SigOp SignalGreaterOrEqual [a, b]
a .>. b = SigOp SignalGreater [a, b]

-- A chain of one operation is one node with many inputs, not a node for every pair.
operands :: SignalOp -> Sig -> [Sig]
operands op (SigOp inner inputs) | inner == op = inputs
operands _ other = [other]

-- | How many of these are true.
countS :: [Sig] -> Sig
countS = SigOp SignalCount

-- | The second if the first is true, otherwise the third.
selectS :: Sig -> Sig -> Sig -> Sig
selectS condition yes no = SigOp SignalSelect [condition, yes, no]

-- | True once the signal has been true for this many seconds without a break.
heldFor :: Double -> Sig -> Sig
heldFor seconds a = SigOp (SignalHeldFor seconds) [a]

-- | Seconds for which the signal has been true: a clock that runs only while it holds.
timer :: Sig -> Sig
timer running = SigOp SignalTimer [running]

-- | A 'timer' that goes back to zero while the second signal is true.
timerReset :: Sig -> Sig -> Sig
timerReset running reset = SigOp SignalTimer [running, reset]

-- | Sums: @sig "a" + sig "b"@. Only addition and literals mean anything for signals; the
-- other operations are errors.
instance Num Sig where
  a + b = SigOp SignalSum (operands SignalSum a ++ operands SignalSum b)
  fromInteger = SigNumber . fromInteger
  (*) = error "Mira.Sig: signals can be added, not multiplied"
  abs = error "Mira.Sig: abs is not a signal operation"
  signum = error "Mira.Sig: signum is not a signal operation"
  negate = error "Mira.Sig: negate is not a signal operation"

-- | Defines a signal by an expression, when this system returns. The parts of the
-- expression become signals of their own, named after the rule (@red.clock#1@, …), so the
-- whole of it shows in the signal graph. Defining the same rule again changes nothing.
defineRule :: System -> String -> Sig -> IO ()
defineRule sys name rule = do
  counter <- newIORef (0 :: Int)
  let fresh = atomicModifyIORef' counter (\n -> (n + 1, name ++ "#" ++ show (n + 1)))
      -- Gives an expression a name in the graph, defining what it needs on the way.
      place target expression = case expression of
        SigNamed other -> defineSignal sys target SignalOr [other]
        SigTruth value -> defineSignal sys target (SignalTruth value) []
        SigNumber value -> defineSignal sys target (SignalNumber value) []
        SigOp op inputs -> mapM input inputs >>= defineSignal sys target op
      input (SigNamed other) = pure other
      input expression = do
        part <- fresh
        place part expression
        pure part
  place name rule

-- What the engine calls for every Haskell system: `user` is the stable pointer to its
-- function. An exception must not escape into the engine, so the engine is told of it instead.
foreign export ccall "mira_hs_dispatch" dispatch :: Ptr () -> Ptr () -> IO ()

dispatch :: Ptr () -> Ptr () -> IO ()
dispatch system user = do
  run <- deRefStablePtr (castPtrToStablePtr user)
  result <- try (run system)
  case result of
    Right () -> pure ()
    Left (err :: SomeException) ->
      withCStringLen (displayException err) $ \(chars, len) ->
        c_system_fail system chars (fromIntegral len)

foreign export ccall "mira_hs_unload" unload :: IO ()

unload :: IO ()
unload = atomicModifyIORef' systems ([],) >>= mapM_ freeStablePtr

-- | A vector of three floats.
data V3 = V3 !Float !Float !Float
  deriving (Eq, Show)

instance Num V3 where
  V3 a b c + V3 x y z = V3 (a + x) (b + y) (c + z)
  V3 a b c - V3 x y z = V3 (a - x) (b - y) (c - z)
  V3 a b c * V3 x y z = V3 (a * x) (b * y) (c * z)
  abs (V3 a b c) = V3 (abs a) (abs b) (abs c)
  signum (V3 a b c) = V3 (signum a) (signum b) (signum c)
  fromInteger n = V3 (fromInteger n) (fromInteger n) (fromInteger n)

instance Storable V3 where
  sizeOf _ = 12
  alignment _ = 4
  peek ptr = V3 <$> peekElemOff (castPtr ptr) 0 <*> peekElemOff (castPtr ptr) 1 <*> peekElemOff (castPtr ptr) 2
  poke ptr (V3 x y z) = pokeElemOff (castPtr ptr) 0 x >> pokeElemOff (castPtr ptr) 1 y >> pokeElemOff (castPtr ptr) 2 z

-- | A unit quaternion: x, y, z, w.
data Quat = Quat !Float !Float !Float !Float
  deriving (Eq, Show)

-- | @mira.Transform@: position, rotation and scale relative to the entity's parent. Laid out
-- as @MiraTransform@ in @mira.h@: 48 bytes, 16-byte aligned.
data Transform = Transform
  { translation :: !V3
  , rotation :: !Quat
  , scale :: !V3
  }
  deriving (Eq, Show)

-- | A transform at the first point, turned so that its forward direction (-Z, the way a
-- camera sees and a light shines) points at the second. Not for looking straight up or down.
lookingAt :: V3 -> V3 -> Transform
lookingAt eye@(V3 ex ey ez) (V3 tx ty tz) = Transform eye orientation (V3 1 1 1)
  where
    unit (x, y, z) = let len = sqrt (x * x + y * y + z * z) in (x / len, y / len, z / len)
    cross (ax, ay, az) (bx, by, bz) = (ay * bz - az * by, az * bx - ax * bz, ax * by - ay * bx)
    forward@(fx, fy, fz) = unit (tx - ex, ty - ey, tz - ez)
    right@(rx, ry, rz) = unit (cross forward (0, 1, 0))
    (ux, uy, uz) = cross right forward
    -- The rotation whose columns are right, up and back, as a quaternion.
    (m00, m01, m02) = (rx, ux, -fx)
    (m10, m11, m12) = (ry, uy, -fy)
    (m20, m21, m22) = (rz, uz, -fz)
    trace = m00 + m11 + m22
    orientation
      | trace > 0 =
          let s = sqrt (trace + 1) * 2
           in Quat ((m21 - m12) / s) ((m02 - m20) / s) ((m10 - m01) / s) (s / 4)
      | m00 > m11 && m00 > m22 =
          let s = sqrt (1 + m00 - m11 - m22) * 2
           in Quat (s / 4) ((m01 + m10) / s) ((m02 + m20) / s) ((m21 - m12) / s)
      | m11 > m22 =
          let s = sqrt (1 + m11 - m00 - m22) * 2
           in Quat ((m01 + m10) / s) (s / 4) ((m12 + m21) / s) ((m02 - m20) / s)
      | otherwise =
          let s = sqrt (1 + m22 - m00 - m11) * 2
           in Quat ((m02 + m20) / s) ((m12 + m21) / s) (s / 4) ((m10 - m01) / s)

instance Storable Transform where
  sizeOf _ = 48
  alignment _ = 16
  peek ptr = do
    t <- peek (castPtr ptr)
    let q = castPtr (ptr `plusPtr` 16) :: Ptr Float
    r <- Quat <$> peekElemOff q 0 <*> peekElemOff q 1 <*> peekElemOff q 2 <*> peekElemOff q 3
    s <- peek (castPtr (ptr `plusPtr` 32))
    pure (Transform t r s)
  poke ptr (Transform t (Quat x y z w) s) = do
    poke (castPtr ptr) t
    let q = castPtr (ptr `plusPtr` 16) :: Ptr Float
    pokeElemOff q 0 x >> pokeElemOff q 1 y >> pokeElemOff q 2 z >> pokeElemOff q 3 w
    poke (castPtr (ptr `plusPtr` 32)) s

-- Sound.

foreign import ccall unsafe "mira_hs_sound_tone"
  c_sound_tone :: Ptr () -> CFloat -> CFloat -> IO Word32
foreign import ccall unsafe "mira_hs_sound_load"
  c_sound_load :: Ptr () -> Ptr CChar -> CSize -> IO Word32
foreign import ccall unsafe "mira_hs_sound_play"
  c_sound_play :: Ptr () -> Word32 -> CFloat -> CFloat -> CFloat -> Word32 -> IO Word64
foreign import ccall unsafe "mira_hs_sound_stop"
  c_sound_stop :: Ptr () -> Word64 -> IO ()
foreign import ccall unsafe "mira_hs_sound_playing"
  c_sound_playing :: Ptr () -> Word64 -> IO Word32
foreign import ccall unsafe "mira_hs_set_sound_source"
  c_set_sound_source :: Ptr () -> Word64 -> Word32 -> CFloat -> CFloat -> Word32 -> IO ()
foreign import ccall unsafe "mira_hs_set_listener"
  c_set_listener :: Ptr () -> Word64 -> IO ()
foreign import ccall unsafe "mira_hs_set_master_volume"
  c_set_master_volume :: Ptr () -> CFloat -> IO ()

-- | A sound: a tone made with 'soundTone', or a file read with 'loadSound'. It can be kept
-- in a component or in state.
newtype Sound = Sound Word32
  deriving (Eq, Show)

instance Storable Sound where
  sizeOf _ = 4
  alignment _ = 4
  peek ptr = Sound <$> peek (castPtr ptr)
  poke ptr (Sound handle) = poke (castPtr ptr) handle

-- | A sound that is playing, from 'playSound'.
newtype Voice = Voice Word64
  deriving (Eq, Show)

instance Storable Voice where
  sizeOf _ = 8
  alignment _ = 8
  peek ptr = Voice <$> peek (castPtr ptr)
  poke ptr (Voice voice) = poke (castPtr ptr) voice

-- | A plain tone: how many hertz, and for how many seconds. The same two numbers give the
-- same sound, however often it is asked for.
soundTone :: System -> Float -> Float -> IO Sound
soundTone (System system) hertz seconds = do
  handle <- c_sound_tone system (realToFrac hertz) (realToFrac seconds)
  when (handle == 0) $ ioError (userError "no tone could be made (an app without sound?)")
  pure (Sound handle)

-- | Loads a sound by name: a WAV or Ogg Vorbis file in the app's asset folder. The same name
-- gives the same sound.
loadSound :: System -> String -> IO Sound
loadSound (System system) name = do
  handle <- withName name (c_sound_load system)
  when (handle == 0) $ ioError (userError ("could not load the sound `" ++ name ++ "`"))
  pure (Sound handle)

-- | Plays a sound at once, this loud (1 is as it was recorded).
playSound :: System -> Sound -> Float -> IO Voice
playSound system sound volume = playSoundWith system sound volume 0 1 False

-- | Plays a sound: how loud, where between the ears (-1 the left, 1 the right), how fast and
-- so how high (1 as recorded, 2 an octave up), and whether it begins again when it ends.
playSoundWith :: System -> Sound -> Float -> Float -> Float -> Bool -> IO Voice
playSoundWith (System system) (Sound sound) volume pan pitch looping =
  Voice <$> c_sound_play system sound (realToFrac volume) (realToFrac pan) (realToFrac pitch) (flag looping)

-- | Stops what is playing, when this system returns.
stopSound :: System -> Voice -> IO ()
stopSound (System system) (Voice voice) = c_sound_stop system voice

-- | Whether it is still playing.
soundPlaying :: System -> Voice -> IO Bool
soundPlaying (System system) (Voice voice) = (/= 0) <$> c_sound_playing system voice

-- | Makes the entity give off a sound from where it is (it also needs a 'Transform'), when
-- this system returns: this loud, heard within this many metres, and going round or not.
setSoundSource :: System -> Entity -> Sound -> Float -> Float -> Bool -> IO ()
setSoundSource (System system) (Entity entity) (Sound sound) volume range looping =
  c_set_sound_source system entity sound (realToFrac volume) (realToFrac range) (flag looping)

-- | Makes the entity the ears that sources are heard by: usually the camera.
setListener :: System -> Entity -> IO ()
setListener (System system) (Entity entity) = c_set_listener system entity

-- | How loud everything is: 1 as it is, 0 silent.
setMasterVolume :: System -> Float -> IO ()
setMasterVolume (System system) volume = c_set_master_volume system (realToFrac volume)

-- Parts of the engine, voxels, the window and the game.

foreign import ccall unsafe "mira_hs_require"
  c_require :: Ptr () -> Ptr CChar -> CSize -> IO CInt
foreign import ccall unsafe "mira_hs_voxel_block"
  c_voxel_block :: Ptr () -> Ptr CChar -> CSize -> IO Word32
foreign import ccall unsafe "mira_hs_voxel_get"
  c_voxel_get :: Ptr () -> Int32 -> Int32 -> Int32 -> IO Word32
foreign import ccall unsafe "mira_hs_voxel_set"
  c_voxel_set :: Ptr () -> Int32 -> Int32 -> Int32 -> Word32 -> IO ()
foreign import ccall unsafe "mira_hs_voxel_solid"
  c_voxel_solid :: Ptr () -> Int32 -> Int32 -> Int32 -> IO Word32
foreign import ccall unsafe "mira_hs_voxel_raycast"
  c_voxel_raycast ::
    Ptr () -> CFloat -> CFloat -> CFloat -> CFloat -> CFloat -> CFloat -> CFloat -> Ptr Int32 -> Ptr CFloat -> Ptr Word32 -> IO Word32
foreign import ccall unsafe "mira_hs_set_chunk_viewer"
  c_set_chunk_viewer :: Ptr () -> Word64 -> IO ()
foreign import ccall unsafe "mira_hs_set_cursor_grabbed"
  c_set_cursor_grabbed :: Ptr () -> Word32 -> IO ()
foreign import ccall unsafe "mira_hs_cursor_grabbed"
  c_cursor_grabbed :: Ptr () -> IO Word32
foreign import ccall unsafe "mira_hs_quit"
  c_quit :: Ptr () -> IO ()

-- | Says the game needs a part of the engine that not every game has: @"voxels"@, terrain of
-- blocks made and streamed around a chunk viewer. Ask the first time the plugin loads.
require :: App -> String -> IO ()
require (App app) part = do
  status <- withName part (c_require app)
  when (status /= 0) $ ioError (userError ("there is no part of the engine called `" ++ part ++ "`"))

-- | A kind of block. 'air' is none at all.
newtype Block = Block Word32
  deriving (Eq, Show)

instance Storable Block where
  sizeOf _ = 4
  alignment _ = 4
  peek ptr = Block <$> peek (castPtr ptr)
  poke ptr (Block kind) = poke (castPtr ptr) kind

air :: Block
air = Block 0

-- | What a ray through the blocks met: the block, the face it met (pointing out of the block;
-- nought if the ray began inside), how far along, and what kind of block it is.
data VoxelHit = VoxelHit
  { hitBlock :: !(Int, Int, Int)
  , hitFace :: !(Int, Int, Int)
  , hitAlong :: !Float
  , hitKind :: !Block
  }
  deriving (Eq, Show)

unknownBlock :: Word32
unknownBlock = 0xFFFFFFFF

known :: Word32 -> Maybe Block
known kind = if kind == unknownBlock then Nothing else Just (Block kind)

-- | The kind of block with a name: @"air"@, @"grass"@, @"dirt"@, @"stone"@, @"sand"@.
voxelBlock :: System -> String -> IO (Maybe Block)
voxelBlock (System system) name = known <$> withName name (c_voxel_block system)

-- | The kind of block at a place, or 'Nothing' where the world is not made yet.
voxel :: System -> (Int, Int, Int) -> IO (Maybe Block)
voxel (System system) (x, y, z) = known <$> c_voxel_get system (fromIntegral x) (fromIntegral y) (fromIntegral z)

-- | Puts a kind of block at a place ('air' takes one away), when this system returns.
setVoxel :: System -> (Int, Int, Int) -> Block -> IO ()
setVoxel (System system) (x, y, z) (Block kind) = c_voxel_set system (fromIntegral x) (fromIntegral y) (fromIntegral z) kind

-- | Whether the block at a place is solid, or the world is not made there yet.
voxelSolid :: System -> (Int, Int, Int) -> IO Bool
voxelSolid (System system) (x, y, z) = (/= 0) <$> c_voxel_solid system (fromIntegral x) (fromIntegral y) (fromIntegral z)

-- | The first solid block along a ray from a point in a direction, within a reach.
voxelRaycast :: System -> V3 -> V3 -> Float -> IO (Maybe VoxelHit)
voxelRaycast (System system) (V3 ox oy oz) (V3 dx dy dz) reach =
  allocaArray 6 $ \cells -> alloca $ \along -> alloca $ \kind -> do
    found <-
      c_voxel_raycast system (realToFrac ox) (realToFrac oy) (realToFrac oz) (realToFrac dx) (realToFrac dy) (realToFrac dz)
        (realToFrac reach) cells along kind
    if found == 0
      then pure Nothing
      else do
        [bx, by, bz, nx, ny, nz] <- map fromIntegral <$> peekArray 6 cells
        distance <- realToFrac <$> peek along
        Just . VoxelHit (bx, by, bz) (nx, ny, nz) distance . Block <$> peek kind

-- | Makes the entity the one the world is made around (it also needs a 'Transform'), when
-- this system returns: usually the camera.
setChunkViewer :: System -> Entity -> IO ()
setChunkViewer (System system) (Entity entity) = c_set_chunk_viewer system entity

-- | Holds the pointer to the window and hides it, for looking about with the mouse, or lets
-- it go.
setCursorGrabbed :: System -> Bool -> IO ()
setCursorGrabbed (System system) grabbed = c_set_cursor_grabbed system (flag grabbed)

-- | Whether the pointer is held.
cursorGrabbed :: System -> IO Bool
cursorGrabbed (System system) = (/= 0) <$> c_cursor_grabbed system

-- | Ends the game after this frame.
quit :: System -> IO ()
quit (System system) = c_quit system
-- | Sets one of the engine's settings by name, as the plugin loads: @view_mode@ (see
-- 'setViewMode'), @ray_tracing@, @multisampling@, @taa@, @bloom@, @vignette@, @grain@,
-- @sharpen@, @exposure@, @contrast@, @saturation@, @render_scale@, @mouse_as_finger@,
-- @swipe_distance@. The graphics are opened with what the first three say.
setting :: App -> String -> Double -> IO ()
setting (App app) name value = do
  status <- withName name $ \chars len -> c_setting app chars len (realToFrac value)
  when (status /= 0) $ ioError (userError ("the setting `" ++ name ++ "` can't be " ++ show value))

-- | How the scene is drawn.
data ViewMode
  = ViewLit
  | -- | Each surface in its own colour, with no light or shadow on it: a flat game.
    ViewUnlit
  | ViewLightingOnly
  | ViewNormals
  | ViewWireframe
  deriving (Eq, Show, Enum, Bounded)

setViewMode :: App -> ViewMode -> IO ()
setViewMode app mode = setting app "view_mode" (fromIntegral (fromEnum mode))

-- | Binds an action, as the plugin loads, to a key, a mouse button, a swipe, a tap or a
-- controller's button by its name (@KeyW@, @MouseLeft@, @SwipeUp@, @Tap@, @PadSouth@),
-- besides what it is bound to already.
bindAction :: App -> String -> String -> IO ()
bindAction (App app) action binding = do
  status <- withName action $ \a alen -> withName binding $ \b blen -> c_action_bind app a alen b blen
  when (status /= 0) $ ioError (userError ("`" ++ action ++ "` can't be bound to `" ++ binding ++ "`"))

-- | Binds an axis to a pair: the first pulls it toward -1, the second toward 1.
bindAxis :: App -> String -> String -> String -> IO ()
bindAxis (App app) axis less more = do
  status <- withName axis $ \a alen -> withName less $ \l llen -> withName more $ \m mlen ->
    c_action_bind_axis app a alen l llen m mlen
  when (status /= 0) $ ioError (userError ("`" ++ axis ++ "` can't be bound to `" ++ less ++ "` and `" ++ more ++ "`"))

-- | What an action is doing: held now, pressed this frame, let go this frame.
data ActionState = ActionState {actionHeld, actionPressed, actionReleased :: !Bool}
  deriving (Eq, Show)

actionState :: System -> String -> IO ActionState
actionState (System system) action = do
  bits <- withName action (c_action_state system)
  pure (ActionState (bits Foreign..&. 1 /= 0) (bits Foreign..&. 2 /= 0) (bits Foreign..&. 4 /= 0))

-- | Where the pointer (the mouse, or else the first finger) points on the level plane at a
-- height, as the camera sees it; 'Nothing' with no pointer or camera.
pointerOnGround :: System -> Float -> IO (Maybe V3)
pointerOnGround (System system) height = allocaArray 3 $ \out -> do
  found <- c_pointer_on_ground system (realToFrac height) out
  if found == 0
    then pure Nothing
    else do
      [x, y, z] <- map realToFrac <$> peekArray 3 out
      pure (Just (V3 x y z))

foreign import ccall unsafe "mira_hs_pointer_on_ground" c_pointer_on_ground :: Ptr () -> CFloat -> Ptr CFloat -> IO Word8

-- | Where an axis is, from -1 to 1.
actionValue :: System -> String -> IO Float
actionValue (System system) axis = realToFrac <$> withName axis (c_action_value system)

-- | Flags for 'createImage': each pixel a crisp square however far it is enlarged (pixel
-- art), and read as plain numbers rather than colour (data such as normals).
pixelated, linearImage :: Word32
pixelated = 1
linearImage = 2

-- | An image made from @width@ x @height@ pixels, four bytes each (red, green, blue, alpha),
-- row by row from the top, kept under a name: the same name gives the same image, its pixels
-- made anew. The flags are 'pixelated' and 'linearImage', added.
createImage :: System -> String -> Int -> Int -> [Word8] -> Word32 -> IO Image
createImage (System system) name width height rgba flags = do
  when (length rgba < width * height * 4) $ ioError (userError ("`" ++ name ++ "` has fewer pixels than its size"))
  handle <- withName name $ \chars len -> withArrayLen rgba $ \_ pixels ->
    c_image_create system chars len (fromIntegral width) (fromIntegral height) pixels flags
  when (handle == 0) $ ioError (userError ("the image `" ++ name ++ "` could not be made"))
  pure (Image handle)

-- | Draws the entity only where its picture's alpha reaches the cutoff, as a sprite cut out of
-- its background; 'Nothing' draws it all. When this system returns.
setAlphaCutoff :: System -> Entity -> Maybe Float -> IO ()
setAlphaCutoff (System system) (Entity entity) cutoff = c_set_alpha_cutoff system entity (maybe (-1) realToFrac cutoff)

-- | A sound made from samples from -1 to 1, so many channels (1 or 2) to a frame, at so many
-- frames a second, kept under a name: the first time it is made, and the same sound after.
createSound :: System -> String -> Int -> Int -> [Float] -> IO Sound
createSound (System system) name rate channels samples = do
  handle <- withName name $ \chars len -> withArrayLen (map realToFrac samples :: [CFloat]) $ \count values ->
    c_sound_create system chars len (fromIntegral rate) (fromIntegral channels) values (fromIntegral count)
  when (handle == 0) $ ioError (userError ("the sound `" ++ name ++ "` could not be made"))
  pure (Sound handle)

-- | Writes a small file into the folder the game's own files are kept in for the person
-- playing, at once and whole: the game's name, then the file's, with no folders in it.
writeGameFile :: System -> String -> String -> [Word8] -> IO ()
writeGameFile (System system) game name bytes = do
  status <- withName game $ \g glen -> withName name $ \n nlen -> withArrayLen bytes $ \count data_ ->
    c_file_write system g glen n nlen data_ (fromIntegral count)
  when (status /= 0) $ ioError (userError ("`" ++ name ++ "` could not be written"))

-- | Reads such a file whole, if there is one.
readGameFile :: System -> String -> String -> IO (Maybe [Word8])
readGameFile (System system) game name =
  withName game $ \g glen -> withName name $ \n nlen -> do
    size <- c_file_read system g glen n nlen Foreign.nullPtr 0
    if size < 0
      then pure Nothing
      else allocaArray (max 1 (fromIntegral size)) $ \out -> do
        got <- c_file_read system g glen n nlen out (fromIntegral size)
        Just <$> peekArray (fromIntegral (min size got)) out

foreign import ccall unsafe "mira_hs_setting" c_setting :: Ptr () -> Ptr CChar -> CSize -> CDouble -> IO CInt
foreign import ccall unsafe "mira_hs_action_bind"
  c_action_bind :: Ptr () -> Ptr CChar -> CSize -> Ptr CChar -> CSize -> IO CInt
foreign import ccall unsafe "mira_hs_action_bind_axis"
  c_action_bind_axis :: Ptr () -> Ptr CChar -> CSize -> Ptr CChar -> CSize -> Ptr CChar -> CSize -> IO CInt
foreign import ccall unsafe "mira_hs_action_state" c_action_state :: Ptr () -> Ptr CChar -> CSize -> IO Word32
foreign import ccall unsafe "mira_hs_action_value" c_action_value :: Ptr () -> Ptr CChar -> CSize -> IO CFloat
foreign import ccall unsafe "mira_hs_image_create"
  c_image_create :: Ptr () -> Ptr CChar -> CSize -> Word32 -> Word32 -> Ptr Word8 -> Word32 -> IO Word32
foreign import ccall unsafe "mira_hs_set_alpha_cutoff" c_set_alpha_cutoff :: Ptr () -> Word64 -> CFloat -> IO ()
foreign import ccall unsafe "mira_hs_sound_create"
  c_sound_create :: Ptr () -> Ptr CChar -> CSize -> Word32 -> Word32 -> Ptr CFloat -> CSize -> IO Word32
foreign import ccall unsafe "mira_hs_file_write"
  c_file_write :: Ptr () -> Ptr CChar -> CSize -> Ptr CChar -> CSize -> Ptr Word8 -> CSize -> IO CInt
foreign import ccall unsafe "mira_hs_file_read"
  c_file_read :: Ptr () -> Ptr CChar -> CSize -> Ptr CChar -> CSize -> Ptr Word8 -> CSize -> IO Int64
