{-# LANGUAGE ForeignFunctionInterface #-}

-- | A world of blocks to fly about in, break and build: the voxel terrain, made and streamed
-- around you, with a ring of spinning cubes above it and balls to throw at it.
--
-- > cargo run --bin mira-play -- examples/haskell/voxel_world
--
-- Click to take the mouse, and look about with it. WASD to fly, Space and Shift to rise and
-- fall, Control to go faster. Left click breaks the block you look at, right click puts one
-- against it; 1 to 4 choose stone, dirt, grass or sand. F throws a ball. Escape lets the
-- mouse go, and a second Escape ends the game.
module World where

import Control.Monad (forM_, unless, when)
import Data.Int (Int32)
import Foreign (Ptr, Storable (..))
import Foreign.C.Types (CInt (..))
import Mira
import Text.Printf (printf)

-- | How far a block can be reached, in metres.
reach :: Float
reach = 8

-- | The blocks 1 to 4 choose.
choices :: [(Key, String)]
choices = [(Digit1, "stone"), (Digit2, "dirt"), (Digit3, "grass"), (Digit4, "sand")]

-- | Turns about an axis, so many radians a second.
data Spin = Spin !V3 !Float

instance Storable Spin where
  sizeOf _ = 16
  alignment _ = 4
  peek ptr = Spin <$> (V3 <$> peekByteOff ptr 0 <*> peekByteOff ptr 4 <*> peekByteOff ptr 8) <*> peekByteOff ptr 12
  poke ptr (Spin (V3 x y z) speed) = mapM_ (\(offset, value) -> pokeByteOff ptr offset value) [(0, x), (4, y), (8, z), (12, speed)]

-- | The camera you fly: how fast, how much the mouse turns it, and where it is looking.
data Flyer = Flyer {flySpeed, sensitivity, yaw, pitch :: !Float}

instance Storable Flyer where
  sizeOf _ = 16
  alignment _ = 4
  peek ptr = Flyer <$> peekByteOff ptr 0 <*> peekByteOff ptr 4 <*> peekByteOff ptr 8 <*> peekByteOff ptr 12
  poke ptr (Flyer speed turning across up) = mapM_ (\(offset, value) -> pokeByteOff ptr offset value) [(0, speed), (4, turning), (8, across), (12, up)]

-- | A thrown ball: how it is moving, how big it is, how much of a bounce it keeps, and the
-- seconds it has left.
data Ball = Ball {velocity :: !V3, radius, restitution, left :: !Float}

instance Storable Ball where
  sizeOf _ = 24
  alignment _ = 4
  peek ptr = Ball <$> (V3 <$> peekByteOff ptr 0 <*> peekByteOff ptr 4 <*> peekByteOff ptr 8) <*> peekByteOff ptr 12 <*> peekByteOff ptr 16 <*> peekByteOff ptr 20
  poke ptr (Ball (V3 x y z) size keep life) =
    mapM_ (\(offset, value) -> pokeByteOff ptr offset value) [(0, x), (4, y), (8, z), (12, size), (16, keep), (20, life)]

foreign export ccall "mira_hs_main" pluginMain :: Ptr () -> IO CInt

pluginMain :: Ptr () -> IO CInt
pluginMain = plugin $ \app -> do
  require app "voxels"
  transform <- lookupComponent app "mira.Transform"
  spin <- registerComponent app "world.Spin"
  flyer <- registerComponent app "world.Flyer"
  ball <- registerComponent app "world.Ball"
  ballMesh <- statePtr app "world.ball" :: IO (Ptr Mesh)
  chosen <- statePtr app "world.chosen" :: IO (Ptr Block)
  counted <- statePtr app "world.counted" :: IO (Ptr Int32)
  since <- statePtr app "world.since" :: IO (Ptr Float)

  addSystem_ app "setup" Startup $ \sys -> do
    cube <- meshCube sys 1
    poke ballMesh =<< meshSphere sys 0.5
    poke chosen . maybe air id =<< voxelBlock sys "stone"

    -- A ring of spinning cubes going round a point above the hills, each placed relative to it.
    pivot <- spawn sys
    insert sys pivot transform (at (V3 28 84 (-28)))
    insert sys pivot spin (Spin (V3 0 1 0) 0.3)
    forM_ [0 .. 11 :: Int] $ \i -> do
      let angle = fromIntegral i / 12 * 2 * pi
      piece <- spawn sys
      insert sys piece transform (at (V3 (cos angle * 10) 0 (sin angle * 10)))
      insert sys piece spin (Spin (unit (V3 (cos angle) 1 (sin angle))) 1.5)
      setMesh sys piece cube
      setMaterial sys piece (material (V3 (0.5 + cos angle * 0.4) 0.5 (0.5 + sin angle * 0.4)))
      setParent sys piece (Just pivot)

    sun <- spawn sys
    insert sys sun transform (lookingAt (V3 0 0 0) (V3 (-0.5) (-1) (-0.3)))
    setLight sys sun defaultLight {lightIntensity = 3}
    setAmbient sys (V3 1 1 1) 0.45

    -- You, looking out over the land: the camera, which the world is made around.
    let looking = towards (V3 1 (-0.35) (-1))
    eye <- spawn sys
    insert sys eye transform (Transform (V3 0 95 0) (turned looking) (V3 1 1 1))
    insert sys eye flyer looking
    setCamera sys eye defaultCamera
    setChunkViewer sys eye

  addSystem app "spin" Update ((,) <$> readC spin <*> write transform) $ \sys _ (Spin axis speed, place) -> do
    dt <- deltaSeconds sys
    modifyRef place $ \t -> t {rotation = rotation t `times` around axis (speed * dt)}

  addSystem app "fly" Update ((,) <$> write flyer <*> write transform) $ \sys _ (me, place) -> do
    grabbed <- cursorGrabbed sys
    escape <- keyPressed sys Escape
    when escape $ if grabbed then setCursorGrabbed sys False else quit sys
    clicked <- mousePressed sys MouseLeft
    when (not grabbed && clicked) $ setCursorGrabbed sys True

    -- Turning with the mouse, while it is held.
    Flyer speed turning across up <- readRef me
    (dx, dy) <- if grabbed then mouseMotion sys else pure (0, 0)
    let me' = Flyer speed turning (across - dx * turning) (max (-1.54) (min 1.54 (up - dy * turning)))
    writeRef me me'
    here <- readRef place
    let facing = turned me'
        forward = facing `rotating` V3 0 0 (-1)
        right = facing `rotating` V3 1 0 0
    held <- mapM (\(key, way) -> (\down -> if down then way else V3 0 0 0) <$> keyDown sys key)
      [(KeyW, forward), (KeyS, scaled (-1) forward), (KeyD, right), (KeyA, scaled (-1) right), (Space, V3 0 1 0), (ShiftLeft, V3 0 (-1) 0)]
    boost <- (\fast -> if fast then 5 else 1) <$> keyDown sys ControlLeft
    dt <- deltaSeconds sys
    let way = foldr plus (V3 0 0 0) held
        moved = if way == V3 0 0 0 then translation here else translation here `plus` scaled (speed * boost * dt) (unit way)
    writeRef place here {translation = moved, rotation = facing}

    -- Breaking and building, at what is looked at.
    breaking <- (grabbed &&) <$> mousePressed sys MouseLeft
    placing <- (grabbed &&) <$> mousePressed sys MouseRight
    when (breaking || placing) $ do
      found <- voxelRaycast sys moved forward reach
      forM_ found $ \(VoxelHit block face _ _) ->
        if breaking
          then setVoxel sys block air
          else unless (face == (0, 0, 0)) $ setVoxel sys (block `beside` face) =<< peek chosen

    forM_ choices $ \(key, name) -> do
      pressed <- keyPressed sys key
      when pressed $ mapM_ (poke chosen) =<< voxelBlock sys name

    throwing <- keyPressed sys KeyF
    when throwing $ do
      thrown <- spawn sys
      insert sys thrown transform (Transform (moved `plus` forward) (Quat 0 0 0 1) (V3 0.6 0.6 0.6))
      insert sys thrown ball (Ball (scaled 30 forward) 0.3 0.6 15)
      setMesh sys thrown =<< peek ballMesh
      setMaterial sys thrown (material (V3 1 0.77 0.24))

  -- Balls fall and bounce off the blocks, an axis at a time: if the leading edge would end up
  -- in a solid block, that axis bounces instead.
  addSystem app "bounce" FixedUpdate ((,) <$> write ball <*> write transform) $ \sys entity (thrown, place) -> do
    dt <- deltaSeconds sys
    Ball v size keep life <- readRef thrown
    if life - dt <= 0
      then despawn sys entity
      else do
        here <- translation <$> readRef place
        let fallen = v `plus` V3 0 (-20 * dt) 0
            step (position, velocity') axis = do
              let speed = component axis velocity'
                  moving = speed * dt
                  probe = setComponent axis (component axis position + moving + size * signum moving) position
              solid <- voxelSolid sys (cell probe)
              pure $
                if solid
                  then (position, bounced axis keep velocity')
                  else (setComponent axis (component axis position + moving) position, velocity')
        (there, after) <- foldlM step (here, fallen) [0, 1, 2]
        writeRef thrown (Ball after size keep (life - dt))
        modifyRef place $ \t -> t {translation = there}

  addSystem_ app "title" Update $ \sys -> do
    dt <- deltaSeconds sys
    frames <- (+ 1) <$> peek counted
    elapsed <- (+ dt) <$> peek since
    if elapsed < 0.5
      then poke counted frames >> poke since elapsed
      else do
        now <- peek chosen
        kinds <- mapM (\(_, name) -> fmap (\kind -> (kind, name)) <$> voxelBlock sys name) choices
        let name = maybe "?" id (lookup now [known | Just known <- kinds])
        setWindowTitle sys (printf "mira: %.0f fps; placing %s (1-4); click: break and place, F: a ball" (fromIntegral frames / elapsed :: Float) name)
        poke counted 0 >> poke since 0

-- What happens to a ball's speed along an axis when it meets a block that way: back, and less
-- of it; meeting the ground, it also rolls less and comes to rest.
bounced :: Int -> Float -> V3 -> V3
bounced axis keep v@(V3 x _ z)
  | axis == 1 =
      let up = -(component 1 v) * keep
       in V3 (x * 0.9) (if abs up < 1 then 0 else up) (z * 0.9)
  | otherwise = setComponent axis (-(component axis v) * keep) v

foldlM :: (b -> a -> IO b) -> b -> [a] -> IO b
foldlM _ start [] = pure start
foldlM f start (x : rest) = f start x >>= \next -> foldlM f next rest

-- Looking in a direction: the yaw and pitch that face it.
towards :: V3 -> Flyer
towards direction = let V3 x y z = unit direction in Flyer 12 0.002 (atan2 (-x) (-z)) (asin y)

-- The rotation of a flyer: turned about the vertical, then tipped up or down.
turned :: Flyer -> Quat
turned (Flyer _ _ across up) = around (V3 0 1 0) across `times` around (V3 1 0 0) up

around :: V3 -> Float -> Quat
around axis angle = let V3 x y z = scaled (sin (angle / 2)) (unit axis) in Quat x y z (cos (angle / 2))

times :: Quat -> Quat -> Quat
times (Quat ax ay az aw) (Quat bx by bz bw) =
  Quat
    (aw * bx + ax * bw + ay * bz - az * by)
    (aw * by - ax * bz + ay * bw + az * bx)
    (aw * bz + ax * by - ay * bx + az * bw)
    (aw * bw - ax * bx - ay * by - az * bz)

rotating :: Quat -> V3 -> V3
rotating q (V3 x y z) = let Quat rx ry rz _ = q `times` Quat x y z 0 `times` conjugate q in V3 rx ry rz
  where
    conjugate (Quat a b c d) = Quat (-a) (-b) (-c) d

plus :: V3 -> V3 -> V3
plus (V3 a b c) (V3 d e f) = V3 (a + d) (b + e) (c + f)

scaled :: Float -> V3 -> V3
scaled s (V3 x y z) = V3 (s * x) (s * y) (s * z)

unit :: V3 -> V3
unit v@(V3 x y z) = let len = sqrt (x * x + y * y + z * z) in if len == 0 then v else scaled (1 / len) v

component :: Int -> V3 -> Float
component axis (V3 x y z) = case axis of 0 -> x; 1 -> y; _ -> z

setComponent :: Int -> Float -> V3 -> V3
setComponent axis value (V3 x y z) = case axis of 0 -> V3 value y z; 1 -> V3 x value z; _ -> V3 x y value

cell :: V3 -> (Int, Int, Int)
cell (V3 x y z) = (floor x, floor y, floor z)

beside :: (Int, Int, Int) -> (Int, Int, Int) -> (Int, Int, Int)
beside (a, b, c) (d, e, f) = (a + d, b + e, c + f)

at :: V3 -> Transform
at position = Transform position (Quat 0 0 0 1) (V3 1 1 1)
